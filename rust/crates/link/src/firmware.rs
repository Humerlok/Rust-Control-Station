//! Firmware service: downloads the manifest and `.apj` images, identifies a board sitting in
//! its bootloader and flashes it. The wire format lives in `control-station-firmware`; this
//! module only does the serial I/O. Flashing can brick a board, so every check that can be done
//! before the erase (board id, flash size, image integrity) is done before the erase.
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use control_station_firmware::{
    boards_compatible, check_status, chunks, is_trusted_url, parse_apj, parse_manifest_gz, request,
    BoardInfo, BootError, Firmware, FirmwareError, Info, ManifestEntry, ManifestError, BL_REV_MAX,
    BL_REV_MIN, MANIFEST_URL, PROG_MULTI_MAX,
};
use control_station_transport::{open, Connection, Endpoint, TransportError};
use serde::Serialize;
use tokio::{
    sync::{broadcast, Mutex as AsyncMutex},
    time::timeout,
};

use crate::{LinkError, LinkManager, LinkState};

#[derive(Debug, Clone, Copy)]
pub struct FirmwareTiming {
    /// Wait for the answer to one request.
    pub io_timeout: Duration,
    /// The chip erase can take many seconds.
    pub erase_timeout: Duration,
    pub sync_tries: u32,
}

impl Default for FirmwareTiming {
    fn default() -> Self {
        Self {
            io_timeout: Duration::from_millis(1000),
            erase_timeout: Duration::from_secs(60),
            sync_tries: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashProgress {
    /// `sync`, `identify`, `erase`, `program`, `verify`, `reboot`, `done`.
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub finished: bool,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum FirmwareServiceError {
    #[error("{0}")]
    Link(#[from] LinkError),
    #[error("{0}")]
    Transport(#[from] TransportError),
    #[error("{0}")]
    Boot(#[from] BootError),
    #[error("{0}")]
    File(#[from] FirmwareError),
    #[error("{0}")]
    Manifest(#[from] ManifestError),
    #[error("another firmware operation is already in progress")]
    Busy,
    #[error("cancelled")]
    Cancelled,
    #[error("disconnect the vehicle first: the serial port must be free for the bootloader")]
    Connected,
    #[error("no answer from the bootloader to {0}; is the board in bootloader mode?")]
    Timeout(&'static str),
    #[error("could not download {0}")]
    Download(String),
    #[error("refusing to download firmware from an untrusted address: {0}")]
    Untrusted(String),
    #[error("unsupported bootloader revision {0} (supported: {BL_REV_MIN} to {BL_REV_MAX})")]
    BootloaderRevision(u32),
    #[error("this firmware is for board {firmware} but the connected board is {board}")]
    WrongBoard { board: u32, firmware: u32 },
    #[error("the firmware has an external flash image, which is not supported yet")]
    ExternalFlash,
    #[error("the bootloader did not report its flash size, so the image cannot be verified")]
    NoFlashSize,
    #[error("the firmware is {image} bytes but the board has only {flash} bytes of flash")]
    TooLarge { image: usize, flash: u32 },
    #[error("verification failed after programming: the board reports CRC {board:#010x}, expected {expected:#010x}; do not reboot the board, flash again")]
    CrcMismatch { board: u32, expected: u32 },
}

struct Inner {
    link: LinkManager,
    progress: broadcast::Sender<FlashProgress>,
    op_lock: AsyncMutex<()>,
    cancel: AtomicBool,
    timing: FirmwareTiming,
}

#[derive(Clone)]
pub struct FirmwareService {
    inner: Arc<Inner>,
}

/// A serial connection with a read buffer, for the request/reply pattern of the bootloader.
struct BootPort {
    conn: Connection,
    buf: Vec<u8>,
    timing: FirmwareTiming,
}

impl BootPort {
    async fn send(&mut self, bytes: &[u8]) -> Result<(), FirmwareServiceError> {
        Ok(self.conn.writer.write(bytes).await?)
    }

    async fn read_exact(
        &mut self,
        n: usize,
        wait: Duration,
        what: &'static str,
    ) -> Result<Vec<u8>, FirmwareServiceError> {
        let deadline = tokio::time::Instant::now() + wait;
        while self.buf.len() < n {
            let mut chunk = [0u8; 256];
            let read = tokio::time::timeout_at(deadline, self.conn.reader.read(&mut chunk))
                .await
                .map_err(|_| FirmwareServiceError::Timeout(what))??;
            self.buf.extend_from_slice(&chunk[..read]);
        }
        Ok(self.buf.drain(..n).collect())
    }

    /// Drops whatever is already waiting on the port (stale telemetry, half replies).
    async fn drain(&mut self) {
        self.buf.clear();
        let mut chunk = [0u8; 256];
        while let Ok(Ok(n)) =
            timeout(Duration::from_millis(30), self.conn.reader.read(&mut chunk)).await
        {
            if n == 0 {
                break;
            }
        }
    }

    async fn status(
        &mut self,
        wait: Duration,
        what: &'static str,
    ) -> Result<(), FirmwareServiceError> {
        let tail = self.read_exact(2, wait, what).await?;
        Ok(check_status(tail[0], tail[1])?)
    }

    /// Sends `frame` and waits for a plain `INSYNC OK`.
    async fn simple(
        &mut self,
        frame: &[u8],
        what: &'static str,
    ) -> Result<(), FirmwareServiceError> {
        self.send(frame).await?;
        self.status(self.timing.io_timeout, what).await
    }

    /// Sends `frame` and reads a 32-bit little-endian value followed by the status.
    async fn word(
        &mut self,
        frame: &[u8],
        what: &'static str,
    ) -> Result<u32, FirmwareServiceError> {
        self.send(frame).await?;
        let value = self.read_exact(4, self.timing.io_timeout, what).await?;
        self.status(self.timing.io_timeout, what).await?;
        Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
    }

    async fn sync(&mut self) -> Result<(), FirmwareServiceError> {
        let mut last = FirmwareServiceError::Timeout("sync");
        for _ in 0..self.timing.sync_tries {
            self.drain().await;
            match self.simple(&request::get_sync(), "sync").await {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    async fn identify(&mut self) -> Result<BoardInfo, FirmwareServiceError> {
        let bootloader_rev = self
            .word(&request::get_device(Info::BlRev), "GET_DEVICE")
            .await?;
        if !(BL_REV_MIN..=BL_REV_MAX).contains(&bootloader_rev) {
            return Err(FirmwareServiceError::BootloaderRevision(bootloader_rev));
        }
        let board_id = self
            .word(&request::get_device(Info::BoardId), "GET_DEVICE")
            .await?;
        let board_rev = self
            .word(&request::get_device(Info::BoardRev), "GET_DEVICE")
            .await?;
        let flash_size = self
            .word(&request::get_device(Info::FlashSize), "GET_DEVICE")
            .await?;
        // Optional queries: older bootloaders answer INVALID or nothing; that must not stop us.
        let extf_size = if bootloader_rev >= 5 {
            self.word(&request::get_device(Info::ExtfSize), "GET_DEVICE")
                .await
                .unwrap_or(0)
        } else {
            0
        };
        let chip = self.word(&request::get_chip(), "GET_CHIP").await.ok();
        let chip_description = self.chip_description().await;
        let serial = self.serial().await;
        if self.sync().await.is_err() {
            // The optional queries may have left the stream out of step; sync once more.
            return Err(FirmwareServiceError::Timeout("sync"));
        }
        Ok(BoardInfo {
            bootloader_rev,
            board_id,
            board_rev,
            flash_size,
            extf_size,
            chip,
            chip_description,
            serial,
        })
    }

    async fn chip_description(&mut self) -> Option<String> {
        self.send(&request::get_chip_des()).await.ok()?;
        let len = self
            .read_exact(4, self.timing.io_timeout, "GET_CHIP_DES")
            .await
            .ok()?;
        let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
        if len > 256 {
            return None;
        }
        let text = self
            .read_exact(len, self.timing.io_timeout, "GET_CHIP_DES")
            .await
            .ok()?;
        self.status(self.timing.io_timeout, "GET_CHIP_DES")
            .await
            .ok()?;
        Some(
            String::from_utf8_lossy(&text)
                .trim_end_matches('\0')
                .trim()
                .to_string(),
        )
    }

    async fn serial(&mut self) -> Option<String> {
        let mut bytes = Vec::new();
        for address in [0u32, 4, 8] {
            let word = self.word(&request::get_sn(address), "GET_SN").await.ok()?;
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        Some(bytes.iter().map(|b| format!("{b:02X}")).collect())
    }
}

impl FirmwareService {
    pub fn new(link: LinkManager) -> Self {
        Self::build(link, FirmwareTiming::default())
    }

    pub fn with_timing(self, timing: FirmwareTiming) -> Self {
        Self::build(self.inner.link.clone(), timing)
    }

    fn build(link: LinkManager, timing: FirmwareTiming) -> Self {
        let (progress, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Inner {
                link,
                progress,
                op_lock: AsyncMutex::new(()),
                cancel: AtomicBool::new(false),
                timing,
            }),
        }
    }

    pub fn subscribe_progress(&self) -> broadcast::Receiver<FlashProgress> {
        self.inner.progress.subscribe()
    }

    /// Stops a flash between two chunks. The board stays in its bootloader, so flashing again works.
    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
    }

    fn emit(&self, phase: &str, done: u64, total: u64, finished: bool, error: Option<String>) {
        let _ = self.inner.progress.send(FlashProgress {
            phase: phase.into(),
            done,
            total,
            finished,
            error,
        });
    }

    /// Downloads the firmware manifest and returns every build in it.
    pub async fn catalog(&self) -> Result<Vec<ManifestEntry>, FirmwareServiceError> {
        let bytes = fetch(MANIFEST_URL.to_string()).await?;
        Ok(parse_manifest_gz(&bytes)?)
    }

    /// Downloads and decodes the `.apj` at `url`; only the official firmware server is accepted.
    pub async fn download(&self, url: &str) -> Result<Firmware, FirmwareServiceError> {
        if !is_trusted_url(url) {
            return Err(FirmwareServiceError::Untrusted(url.to_string()));
        }
        let bytes = fetch(url.to_string()).await?;
        Ok(parse_apj(&bytes)?)
    }

    /// Tells the connected vehicle to reboot into its bootloader and drops the link, which
    /// frees the serial port for [`Self::identify`] and [`Self::flash`].
    pub async fn reboot_to_bootloader(&self) -> Result<(), FirmwareServiceError> {
        self.inner.link.reboot_to_bootloader().await?;
        self.inner.link.disconnect();
        Ok(())
    }

    fn require_free_port(&self) -> Result<(), FirmwareServiceError> {
        if self.inner.link.link_info().state != LinkState::Disconnected {
            return Err(FirmwareServiceError::Connected);
        }
        Ok(())
    }

    async fn open_port(&self, port: &str) -> Result<BootPort, FirmwareServiceError> {
        self.require_free_port()?;
        let conn = open(&Endpoint::Serial {
            port: port.to_string(),
            baud: 115_200,
        })
        .await?;
        Ok(BootPort {
            conn,
            buf: Vec::new(),
            timing: self.inner.timing,
        })
    }

    /// Reads what the bootloader on `port` says about the board. Changes nothing on it.
    pub async fn identify(&self, port: &str) -> Result<BoardInfo, FirmwareServiceError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| FirmwareServiceError::Busy)?;
        let mut boot = self.open_port(port).await?;
        self.identify_on(&mut boot).await
    }

    async fn identify_on(&self, boot: &mut BootPort) -> Result<BoardInfo, FirmwareServiceError> {
        self.emit("sync", 0, 0, false, None);
        boot.sync().await?;
        self.emit("identify", 0, 0, false, None);
        let info = boot.identify().await?;
        self.emit("identify", 0, 0, true, None);
        Ok(info)
    }

    /// Programs `firmware` into the board whose bootloader is on `port`.
    pub async fn flash(
        &self,
        port: &str,
        firmware: &Firmware,
    ) -> Result<BoardInfo, FirmwareServiceError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| FirmwareServiceError::Busy)?;
        let boot = self.open_port(port).await?;
        self.flash_on(boot, firmware).await
    }

    /// [`Self::flash`] over an already open connection (tests use an in-memory one).
    pub async fn flash_connection(
        &self,
        conn: Connection,
        firmware: &Firmware,
    ) -> Result<BoardInfo, FirmwareServiceError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| FirmwareServiceError::Busy)?;
        let boot = BootPort {
            conn,
            buf: Vec::new(),
            timing: self.inner.timing,
        };
        self.flash_on(boot, firmware).await
    }

    async fn flash_on(
        &self,
        mut boot: BootPort,
        firmware: &Firmware,
    ) -> Result<BoardInfo, FirmwareServiceError> {
        self.inner.cancel.store(false, Ordering::SeqCst);
        let result = self.run_flash(&mut boot, firmware).await;
        match &result {
            Ok(_) => self.emit("done", 1, 1, true, None),
            Err(e) => self.emit("done", 0, 0, true, Some(e.to_string())),
        }
        result
    }

    async fn run_flash(
        &self,
        boot: &mut BootPort,
        firmware: &Firmware,
    ) -> Result<BoardInfo, FirmwareServiceError> {
        let info = self.identify_on(boot).await?;
        // Everything below that can refuse does so before the erase.
        if !boards_compatible(info.board_id, firmware.board_id) {
            return Err(FirmwareServiceError::WrongBoard {
                board: info.board_id,
                firmware: firmware.board_id,
            });
        }
        if firmware.extf_image_size > 0 {
            return Err(FirmwareServiceError::ExternalFlash);
        }
        if info.flash_size == 0 {
            return Err(FirmwareServiceError::NoFlashSize);
        }
        if firmware.image.len() > info.flash_size as usize {
            return Err(FirmwareServiceError::TooLarge {
                image: firmware.image.len(),
                flash: info.flash_size,
            });
        }
        self.check_cancel()?;

        self.emit("erase", 0, 1, false, None);
        boot.send(&request::chip_erase()).await?;
        boot.status(self.inner.timing.erase_timeout, "CHIP_ERASE")
            .await?;
        self.emit("erase", 1, 1, true, None);

        let total = firmware.image.len() as u64;
        let mut done = 0u64;
        for piece in chunks(&firmware.image, PROG_MULTI_MAX) {
            self.check_cancel()?;
            boot.simple(&request::prog_multi(piece), "PROG_MULTI")
                .await?;
            done += piece.len() as u64;
            self.emit("program", done, total, false, None);
        }
        self.emit("program", total, total, true, None);

        self.emit("verify", 0, 1, false, None);
        let expected = firmware.crc(info.flash_size);
        let board = boot.word(&request::get_crc(), "GET_CRC").await?;
        if board != expected {
            return Err(FirmwareServiceError::CrcMismatch { board, expected });
        }
        self.emit("verify", 1, 1, true, None);

        self.emit("reboot", 0, 1, false, None);
        // The board resets right away and may never send the status.
        let _ = boot.simple(&request::reboot(), "REBOOT").await;
        Ok(info)
    }

    fn check_cancel(&self) -> Result<(), FirmwareServiceError> {
        if self.inner.cancel.load(Ordering::SeqCst) {
            return Err(FirmwareServiceError::Cancelled);
        }
        Ok(())
    }
}

async fn fetch(url: String) -> Result<Vec<u8>, FirmwareServiceError> {
    let shown = url.clone();
    tokio::task::spawn_blocking(move || control_station_ardupilot_meta::http_fetch(&url))
        .await
        .map_err(|_| FirmwareServiceError::Download(shown.clone()))?
        .map_err(|e| FirmwareServiceError::Download(format!("{shown}: {e}")))
}
