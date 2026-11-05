//! MAVFTP client (file transfer over FILE_TRANSFER_PROTOCOL) as a service next to
//! [`LinkManager`], using only its public API. The wire format is in `control-station-mavftp`.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
        Arc,
    },
    time::Duration,
};

use control_station_mavftp::{
    crc32, errno_name, parse_listing, DirEntry, Frame, FtpError, Opcode, MAX_DATA,
};
use control_station_mavlink::{dialect::FILE_TRANSFER_PROTOCOL_DATA, MavMessage, Packet};
use control_station_params::{unpack_param_pck, Param, PckError};
use control_station_vehicle_state::VehicleId;
use serde::Serialize;
use tokio::{
    sync::{broadcast, Mutex as AsyncMutex},
    time::{timeout_at, Instant},
};

use crate::{LinkError, LinkManager};

/// The fast parameter file. `withdefaults=1` is honoured by recent firmware and ignored by old.
pub const PARAM_PCK_PATH: &str = "@PARAM/param.pck?withdefaults=1";

#[derive(Debug, Clone, Copy)]
pub struct FtpTiming {
    /// Wait for the answer to one request, per try.
    pub request_timeout: Duration,
    pub tries: u32,
    /// A burst read that goes quiet this long is restarted from the first missing byte.
    pub burst_idle: Duration,
    /// Bursts in a row that deliver nothing new before giving up.
    pub burst_tries: u32,
    /// Bytes per read request.
    pub read_chunk: usize,
    /// Bytes per write request (small by default, like Mission Planner, for slow radios).
    pub write_chunk: usize,
}

impl Default for FtpTiming {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_millis(1500),
            tries: 5,
            burst_idle: Duration::from_millis(1500),
            burst_tries: 5,
            read_chunk: 110,
            write_chunk: 80,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FtpProgress {
    /// `list`, `read`, `write`, `remove`, ...
    pub op: String,
    pub path: String,
    pub done: u64,
    pub total: u64,
    pub finished: bool,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum FtpServiceError {
    #[error("{0}")]
    Link(#[from] LinkError),
    #[error("another file transfer is already in progress")]
    Busy,
    #[error("cancelled")]
    Cancelled,
    #[error("no answer from the vehicle to {0} (does it support MAVFTP?)")]
    Timeout(&'static str),
    #[error("{op} {path}: {error}")]
    Refused {
        op: &'static str,
        path: String,
        error: FtpError,
    },
    #[error("invalid path: {0}")]
    BadPath(String),
    #[error("unexpected reply from the vehicle: {0}")]
    Protocol(String),
    #[error("CRC mismatch after upload: sent {local:#010x}, vehicle has {remote:#010x}")]
    CrcMismatch { local: u32, remote: u32 },
    #[error("fast parameter download failed: {0}")]
    ParamFile(#[from] PckError),
}

struct Inner {
    link: LinkManager,
    progress: broadcast::Sender<FtpProgress>,
    op_lock: AsyncMutex<()>,
    seq: AtomicU16,
    cancel: AtomicBool,
    /// Set once a vehicle NAKs ListDirectoryWithTime; later listings skip straight to the plain one.
    no_timed_listing: AtomicBool,
    timing: FtpTiming,
}

#[derive(Clone)]
pub struct FtpService {
    inner: Arc<Inner>,
}

type Packets = broadcast::Receiver<Arc<Packet>>;

/// Everything one operation needs; created after the lock is held.
struct Op {
    name: &'static str,
    path: String,
    vehicle: VehicleId,
    packets: Packets,
}

impl FtpService {
    pub fn new(link: LinkManager) -> Self {
        Self::build(link, FtpTiming::default())
    }

    pub fn with_timing(self, timing: FtpTiming) -> Self {
        Self::build(self.inner.link.clone(), timing)
    }

    fn build(link: LinkManager, timing: FtpTiming) -> Self {
        let (progress, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Inner {
                link,
                progress,
                op_lock: AsyncMutex::new(()),
                seq: AtomicU16::new(0),
                cancel: AtomicBool::new(false),
                no_timed_listing: AtomicBool::new(false),
                timing,
            }),
        }
    }

    pub fn subscribe_progress(&self) -> broadcast::Receiver<FtpProgress> {
        self.inner.progress.subscribe()
    }

    /// Asks the running transfer to stop at the next opportunity.
    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
    }

    fn emit(&self, op: &Op, done: u64, total: u64, finished: bool, error: Option<String>) {
        self.emit_for(op.name, &op.path, done, total, finished, error);
    }

    fn emit_for(
        &self,
        name: &str,
        path: &str,
        done: u64,
        total: u64,
        finished: bool,
        error: Option<String>,
    ) {
        let _ = self.inner.progress.send(FtpProgress {
            op: name.to_string(),
            path: path.to_string(),
            done,
            total,
            finished,
            error,
        });
    }

    fn check_cancel(&self) -> Result<(), FtpServiceError> {
        if self.inner.cancel.load(Ordering::SeqCst) {
            Err(FtpServiceError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Runs `body` as the one active operation, reporting start and finish.
    async fn run<T, F, Fut>(
        &self,
        name: &'static str,
        path: &str,
        body: F,
    ) -> Result<T, FtpServiceError>
    where
        F: FnOnce(Op) -> Fut,
        Fut: std::future::Future<Output = Result<T, FtpServiceError>>,
    {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| FtpServiceError::Busy)?;
        self.inner.cancel.store(false, Ordering::SeqCst);
        let (vehicle, _) = self.inner.link.selected_vehicle()?;
        let op = Op {
            name,
            path: path.to_string(),
            vehicle,
            packets: self.inner.link.subscribe_packets(),
        };
        self.emit_for(name, path, 0, 0, false, None);
        let result = body(op).await;
        match &result {
            Ok(_) => self.emit_for(name, path, 1, 1, true, None),
            Err(error) => self.emit_for(name, path, 0, 0, true, Some(error.to_string())),
        }
        result
    }

    fn next_seq(&self) -> u16 {
        self.inner.seq.fetch_add(1, Ordering::Relaxed)
    }

    fn send_frame(&self, op: &Op, frame: &Frame) -> Result<(), FtpServiceError> {
        let payload = frame
            .encode()
            .map_err(|e| FtpServiceError::Protocol(e.to_string()))?;
        self.inner.link.send(&MavMessage::FILE_TRANSFER_PROTOCOL(
            FILE_TRANSFER_PROTOCOL_DATA {
                target_network: 0,
                target_system: op.vehicle.system_id,
                target_component: op.vehicle.component_id,
                payload,
            },
        ))?;
        Ok(())
    }

    /// The next MAVFTP frame from the vehicle before `deadline`.
    async fn next_frame(op: &mut Op, deadline: Instant) -> Option<Frame> {
        loop {
            match timeout_at(deadline, op.packets.recv()).await {
                Err(_) | Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Ok(packet)) => {
                    if packet.system_id() != op.vehicle.system_id
                        || packet.component_id() != op.vehicle.component_id
                    {
                        continue;
                    }
                    if let MavMessage::FILE_TRANSFER_PROTOCOL(data) = &packet.message {
                        if let Ok(frame) = Frame::decode(&data.payload) {
                            return Some(frame);
                        }
                    }
                }
            }
        }
    }

    /// Sends `request` and returns its ACK, resending on silence. A NAK for this opcode is an
    /// error (except that the caller sees `Eof`/`UnknownCommand` as such, not retried).
    async fn call(&self, op: &mut Op, request: Frame) -> Result<Frame, FtpServiceError> {
        let timing = self.inner.timing;
        let request = Frame {
            seq: self.next_seq(),
            ..request
        };
        for _ in 0..timing.tries {
            self.check_cancel()?;
            self.send_frame(op, &request)?;
            let deadline = Instant::now() + timing.request_timeout;
            while let Some(frame) = Self::next_frame(op, deadline).await {
                if frame.req_opcode != request.opcode {
                    continue;
                }
                match frame.opcode {
                    Opcode::Nak => {
                        return Err(FtpServiceError::Refused {
                            op: op.name,
                            path: op.path.clone(),
                            error: frame.nak().unwrap_or(FtpError::Fail),
                        })
                    }
                    Opcode::Ack if frame.seq == request.seq.wrapping_add(1) => return Ok(frame),
                    _ => {}
                }
            }
        }
        Err(FtpServiceError::Timeout(op.name))
    }

    fn check_path(path: &str) -> Result<(), FtpServiceError> {
        if path.is_empty() {
            return Err(FtpServiceError::BadPath("empty path".into()));
        }
        if path.contains('\0') {
            return Err(FtpServiceError::BadPath("contains a NUL byte".into()));
        }
        if path.len() > MAX_DATA - 1 {
            return Err(FtpServiceError::BadPath(format!(
                "longer than {} bytes",
                MAX_DATA - 1
            )));
        }
        Ok(())
    }

    /// Lists one directory (not recursive). Modification times are included when the firmware
    /// supports them.
    pub async fn list(&self, dir: &str) -> Result<Vec<DirEntry>, FtpServiceError> {
        Self::check_path(dir)?;
        let dir = if dir.len() > 1 {
            dir.trim_end_matches('/')
        } else {
            dir
        };
        self.run("list", dir, |mut op| async move {
            if !self.inner.no_timed_listing.load(Ordering::SeqCst) {
                match self
                    .list_with(&mut op, dir, Opcode::ListDirectoryWithTime)
                    .await
                {
                    Err(FtpServiceError::Refused {
                        error: FtpError::UnknownCommand | FtpError::Fail,
                        ..
                    }) => self.inner.no_timed_listing.store(true, Ordering::SeqCst),
                    other => return other,
                }
            }
            self.list_with(&mut op, dir, Opcode::ListDirectory).await
        })
        .await
    }

    async fn list_with(
        &self,
        op: &mut Op,
        dir: &str,
        opcode: Opcode,
    ) -> Result<Vec<DirEntry>, FtpServiceError> {
        let with_time = opcode == Opcode::ListDirectoryWithTime;
        let mut entries = Vec::new();
        let mut records = 0usize;
        loop {
            let request = Frame::request(0, opcode)
                .with_data(dir.as_bytes().to_vec())
                .with_offset(records as u32);
            match self.call(op, request).await {
                Ok(frame) => {
                    let (page, count) = parse_listing(&frame.data, with_time);
                    if count == 0 {
                        return Ok(entries);
                    }
                    records += count;
                    entries.extend(page);
                }
                Err(FtpServiceError::Refused {
                    error: FtpError::Eof,
                    ..
                }) => return Ok(entries),
                // a NAK on the very first page of a timed listing means "not supported"
                Err(e) => return Err(e),
            }
        }
    }

    /// Downloads a whole file. Uses burst reads (the vehicle streams chunks) and re-requests
    /// from the first missing byte when chunks are lost.
    pub async fn read(&self, path: &str) -> Result<Vec<u8>, FtpServiceError> {
        Self::check_path(path)?;
        self.run("read", path, |mut op| async move {
            self.read_file(&mut op, path).await
        })
        .await
    }

    async fn read_file(&self, op: &mut Op, path: &str) -> Result<Vec<u8>, FtpServiceError> {
        let open = match self
            .call(
                op,
                Frame::request(0, Opcode::OpenFileRO).with_data(path.as_bytes().to_vec()),
            )
            .await
        {
            Err(FtpServiceError::Refused {
                error: FtpError::NoSessionsAvailable,
                ..
            }) => {
                self.call(op, Frame::request(0, Opcode::ResetSessions))
                    .await?;
                self.call(
                    op,
                    Frame::request(0, Opcode::OpenFileRO).with_data(path.as_bytes().to_vec()),
                )
                .await?
            }
            other => other?,
        };
        let session = open.session;
        let size = open
            .data
            .get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| FtpServiceError::Protocol("open reply without a file size".into()))?;
        let result = self.read_session(op, session, size).await;
        // best effort: free the session whatever happened
        let _ = self
            .call(
                op,
                Frame::request(0, Opcode::TerminateSession).with_session(session),
            )
            .await;
        result
    }

    async fn read_session(
        &self,
        op: &mut Op,
        session: u8,
        size: usize,
    ) -> Result<Vec<u8>, FtpServiceError> {
        let timing = self.inner.timing;
        let chunk = timing.read_chunk.clamp(1, MAX_DATA);
        let mut buf = vec![0u8; size];
        // start offset -> length of every chunk received so far
        let mut chunks: HashMap<usize, usize> = HashMap::new();
        let mut next = 0usize;
        let mut stalled = 0;
        let mut burst_supported = true;
        while next < size {
            self.check_cancel()?;
            self.emit(op, next as u64, size as u64, false, None);
            let before = next;
            if burst_supported {
                let request = Frame {
                    seq: self.next_seq(),
                    ..Frame::request(0, Opcode::BurstReadFile)
                        .with_session(session)
                        .with_offset(next as u32)
                        .with_size(chunk as u8)
                };
                self.send_frame(op, &request)?;
                let mut deadline = Instant::now() + timing.burst_idle;
                while let Some(frame) = Self::next_frame(op, deadline).await {
                    self.check_cancel()?;
                    if frame.req_opcode != Opcode::BurstReadFile {
                        continue;
                    }
                    match frame.opcode {
                        Opcode::Nak => match frame.nak().unwrap_or(FtpError::Fail) {
                            FtpError::Eof => break,
                            FtpError::UnknownCommand => {
                                burst_supported = false;
                                break;
                            }
                            error => {
                                return Err(FtpServiceError::Refused {
                                    op: op.name,
                                    path: op.path.clone(),
                                    error,
                                })
                            }
                        },
                        Opcode::Ack => {
                            let offset = frame.offset as usize;
                            if frame.session == session && offset < size && !frame.data.is_empty() {
                                let len = frame.data.len().min(size - offset);
                                buf[offset..offset + len].copy_from_slice(&frame.data[..len]);
                                chunks.insert(offset, len);
                            }
                            deadline = Instant::now() + timing.burst_idle;
                            if frame.burst_complete {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                let reply = self
                    .call(
                        op,
                        Frame::request(0, Opcode::ReadFile)
                            .with_session(session)
                            .with_offset(next as u32)
                            .with_size(chunk as u8),
                    )
                    .await?;
                let len = reply.data.len().min(size - next);
                buf[next..next + len].copy_from_slice(&reply.data[..len]);
                chunks.insert(next, len);
            }
            while let Some(len) = chunks.get(&next) {
                next += len;
            }
            if next == before {
                stalled += 1;
                if stalled >= timing.burst_tries {
                    return Err(FtpServiceError::Timeout("read"));
                }
            } else {
                stalled = 0;
            }
        }
        Ok(buf)
    }

    /// Uploads `data` to `path` (replacing it) and, when `verify` is set, checks the CRC32 the
    /// vehicle computes for the stored file.
    pub async fn write(
        &self,
        path: &str,
        data: &[u8],
        verify: bool,
    ) -> Result<(), FtpServiceError> {
        Self::check_path(path)?;
        self.run("write", path, |mut op| async move {
            let timing = self.inner.timing;
            let chunk = timing.write_chunk.clamp(1, MAX_DATA);
            let created = match self
                .call(
                    &mut op,
                    Frame::request(0, Opcode::CreateFile).with_data(path.as_bytes().to_vec()),
                )
                .await
            {
                Err(FtpServiceError::Refused {
                    error: FtpError::NoSessionsAvailable,
                    ..
                }) => {
                    self.call(&mut op, Frame::request(0, Opcode::ResetSessions))
                        .await?;
                    self.call(
                        &mut op,
                        Frame::request(0, Opcode::CreateFile).with_data(path.as_bytes().to_vec()),
                    )
                    .await?
                }
                other => other?,
            };
            let session = created.session;
            let result = async {
                let mut offset = 0usize;
                while offset < data.len() {
                    self.check_cancel()?;
                    self.emit(&op, offset as u64, data.len() as u64, false, None);
                    let end = (offset + chunk).min(data.len());
                    self.call(
                        &mut op,
                        Frame::request(0, Opcode::WriteFile)
                            .with_session(session)
                            .with_offset(offset as u32)
                            .with_data(data[offset..end].to_vec()),
                    )
                    .await?;
                    offset = end;
                }
                Ok::<_, FtpServiceError>(())
            }
            .await;
            let _ = self
                .call(
                    &mut op,
                    Frame::request(0, Opcode::TerminateSession).with_session(session),
                )
                .await;
            result?;
            if verify {
                let remote = self.remote_crc(&mut op, path).await?;
                let local = crc32(0, data);
                if remote != local {
                    return Err(FtpServiceError::CrcMismatch { local, remote });
                }
            }
            Ok(())
        })
        .await
    }

    async fn remote_crc(&self, op: &mut Op, path: &str) -> Result<u32, FtpServiceError> {
        let reply = self
            .call(
                op,
                Frame::request(0, Opcode::CalcFileCRC32).with_data(path.as_bytes().to_vec()),
            )
            .await?;
        reply
            .data
            .get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| FtpServiceError::Protocol("CRC reply without a value".into()))
    }

    /// The CRC32 (ArduPilot variant) of a file stored on the vehicle.
    pub async fn crc32(&self, path: &str) -> Result<u32, FtpServiceError> {
        Self::check_path(path)?;
        self.run("crc", path, |mut op| async move {
            self.remote_crc(&mut op, path).await
        })
        .await
    }

    async fn simple(
        &self,
        name: &'static str,
        opcode: Opcode,
        path: &str,
        data: Vec<u8>,
    ) -> Result<(), FtpServiceError> {
        Self::check_path(path)?;
        self.run(name, path, |mut op| async move {
            self.call(&mut op, Frame::request(0, opcode).with_data(data))
                .await
                .map(|_| ())
        })
        .await
    }

    pub async fn remove(&self, path: &str) -> Result<(), FtpServiceError> {
        self.simple("remove", Opcode::RemoveFile, path, path.as_bytes().to_vec())
            .await
    }

    pub async fn mkdir(&self, path: &str) -> Result<(), FtpServiceError> {
        self.simple(
            "mkdir",
            Opcode::CreateDirectory,
            path,
            path.as_bytes().to_vec(),
        )
        .await
    }

    /// Removes an empty directory.
    pub async fn rmdir(&self, path: &str) -> Result<(), FtpServiceError> {
        self.simple(
            "rmdir",
            Opcode::RemoveDirectory,
            path,
            path.as_bytes().to_vec(),
        )
        .await
    }

    pub async fn rename(&self, from: &str, to: &str) -> Result<(), FtpServiceError> {
        Self::check_path(to)?;
        let mut data = from.as_bytes().to_vec();
        data.push(0);
        data.extend_from_slice(to.as_bytes());
        data.push(0);
        if data.len() > MAX_DATA {
            return Err(FtpServiceError::BadPath(
                "both paths together are too long".into(),
            ));
        }
        self.simple("rename", Opcode::Rename, from, data).await
    }

    /// Downloads every parameter through `@PARAM/param.pck`, far faster than PARAM_REQUEST_LIST.
    pub async fn read_params(&self) -> Result<Vec<Param>, FtpServiceError> {
        let bytes = self.read(PARAM_PCK_PATH).await?;
        Ok(unpack_param_pck(&bytes)?)
    }
}

/// Human text for a FailErrno code, used by the UI.
pub fn describe_errno(errno: u8) -> String {
    format!("{} ({errno})", errno_name(errno))
}
