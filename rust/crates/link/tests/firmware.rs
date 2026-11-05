//! Flashing against a scripted in-memory bootloader: the happy path, and every refusal that
//! must happen before the chip erase.
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use control_station_firmware::{crc32_pad, reply, Firmware, EOC};
use control_station_link::{FirmwareService, FirmwareServiceError, FirmwareTiming, LinkManager};
use control_station_transport::Connection;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

#[derive(Clone)]
struct Board {
    board_id: u32,
    flash_size: u32,
    /// Flip a byte of what is stored so the CRC check must fail.
    corrupt: bool,
}

#[derive(Default)]
struct Seen {
    erased: bool,
    rebooted: bool,
    flash: Vec<u8>,
}

fn word(value: u32) -> Vec<u8> {
    let mut v = value.to_le_bytes().to_vec();
    v.extend([reply::INSYNC, reply::OK]);
    v
}

/// Serves the bootloader protocol on one end of a duplex pipe.
fn fake_bootloader(board: Board, seen: Arc<Mutex<Seen>>) -> Connection {
    let (gcs, device) = duplex(4096);
    let (gcs_r, gcs_w) = tokio::io::split(gcs);
    tokio::spawn(async move {
        let (mut r, mut w) = tokio::io::split(device);
        let ok = vec![reply::INSYNC, reply::OK];
        loop {
            let mut op = [0u8; 1];
            if r.read_exact(&mut op).await.is_err() {
                return;
            }
            let answer = match op[0] {
                0x21 => {
                    r.read_exact(&mut [0u8; 1]).await.unwrap();
                    ok.clone()
                }
                0x22 => {
                    let mut b = [0u8; 2];
                    r.read_exact(&mut b).await.unwrap();
                    match b[0] {
                        1 => word(5),
                        2 => word(board.board_id),
                        3 => word(0),
                        4 => word(board.flash_size),
                        _ => vec![reply::INSYNC, reply::INVALID],
                    }
                }
                0x2c | 0x2e | 0x2b => {
                    // Optional queries: this bootloader does not know them.
                    let extra = if op[0] == 0x2b { 5 } else { 1 };
                    r.read_exact(&mut vec![0u8; extra]).await.unwrap();
                    vec![reply::INSYNC, reply::INVALID]
                }
                0x23 => {
                    r.read_exact(&mut [0u8; 1]).await.unwrap();
                    seen.lock().unwrap().erased = true;
                    ok.clone()
                }
                0x27 => {
                    let mut len = [0u8; 1];
                    r.read_exact(&mut len).await.unwrap();
                    let mut data = vec![0u8; len[0] as usize + 1];
                    r.read_exact(&mut data).await.unwrap();
                    assert_eq!(data.pop(), Some(EOC));
                    seen.lock().unwrap().flash.extend(data);
                    ok.clone()
                }
                0x29 => {
                    r.read_exact(&mut [0u8; 1]).await.unwrap();
                    let mut stored = seen.lock().unwrap().flash.clone();
                    if board.corrupt {
                        stored[0] ^= 0xFF;
                    }
                    word(crc32_pad(&stored, board.flash_size))
                }
                0x30 => {
                    r.read_exact(&mut [0u8; 1]).await.unwrap();
                    seen.lock().unwrap().rebooted = true;
                    ok.clone()
                }
                other => panic!("unexpected request {other:#04x}"),
            };
            w.write_all(&answer).await.unwrap();
        }
    });
    Connection::from_io(gcs_r, gcs_w, "fake bootloader")
}

/// Like `parse_apj`, the image is padded with 0xFF to a multiple of 4 bytes.
fn firmware(board_id: u32, len: usize) -> Firmware {
    let mut image: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    while !image.len().is_multiple_of(4) {
        image.push(0xFF);
    }
    Firmware {
        board_id,
        description: "test".into(),
        version: "4.5.0".into(),
        git_hash: String::new(),
        image,
        image_size: len,
        extf_image_size: 0,
    }
}

fn service() -> FirmwareService {
    FirmwareService::new(LinkManager::new()).with_timing(FirmwareTiming {
        io_timeout: Duration::from_millis(300),
        erase_timeout: Duration::from_secs(2),
        sync_tries: 2,
    })
}

fn board() -> Board {
    Board {
        board_id: 140,
        flash_size: 2048,
        corrupt: false,
    }
}

#[tokio::test]
async fn flashes_verifies_and_reboots() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let service = service();
    let mut progress = service.subscribe_progress();
    let fw = firmware(140, 1001);
    let info = service
        .flash_connection(fake_bootloader(board(), seen.clone()), &fw)
        .await
        .unwrap();
    assert_eq!(
        (info.board_id, info.flash_size, info.bootloader_rev),
        (140, 2048, 5)
    );
    let seen = seen.lock().unwrap();
    assert!(seen.erased && seen.rebooted);
    assert_eq!(
        seen.flash, fw.image,
        "image reaches the board byte for byte (padded to 4)"
    );
    let mut last = None;
    while let Ok(p) = progress.try_recv() {
        last = Some(p);
    }
    let last = last.unwrap();
    assert_eq!(
        (last.phase.as_str(), last.finished, last.error),
        ("done", true, None)
    );
}

#[tokio::test]
async fn wrong_board_is_refused_before_the_erase() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let err = service()
        .flash_connection(fake_bootloader(board(), seen.clone()), &firmware(9, 100))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        FirmwareServiceError::WrongBoard {
            board: 140,
            firmware: 9
        }
    ));
    assert!(!seen.lock().unwrap().erased);
}

#[tokio::test]
async fn oversized_image_is_refused_before_the_erase() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let err = service()
        .flash_connection(fake_bootloader(board(), seen.clone()), &firmware(140, 4096))
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareServiceError::TooLarge { .. }));
    assert!(!seen.lock().unwrap().erased);
}

#[tokio::test]
async fn crc_mismatch_is_reported_and_the_board_is_not_rebooted() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let bad = Board {
        corrupt: true,
        ..board()
    };
    let err = service()
        .flash_connection(fake_bootloader(bad, seen.clone()), &firmware(140, 500))
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareServiceError::CrcMismatch { .. }));
    assert!(!seen.lock().unwrap().rebooted);
}

#[tokio::test]
async fn silence_is_a_clear_timeout() {
    let (gcs, _device) = duplex(64);
    let (r, w) = tokio::io::split(gcs);
    let err = service()
        .flash_connection(Connection::from_io(r, w, "silent"), &firmware(140, 100))
        .await
        .unwrap_err();
    assert!(
        matches!(err, FirmwareServiceError::Timeout("sync")),
        "{err}"
    );
}

#[tokio::test]
async fn untrusted_download_urls_are_refused() {
    let err = service()
        .download("https://evil.example/fw.apj")
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareServiceError::Untrusted(_)));
}
