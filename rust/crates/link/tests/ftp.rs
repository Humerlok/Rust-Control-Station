//! MAVFTP transfers against a scripted fake vehicle with an in-memory file system, over UDP
//! loopback: listing, burst reads with lost chunks, verified uploads and file operations.
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_link::{
    ConnectionConfig, FtpError, FtpService, FtpServiceError, FtpTiming, LinkManager, ParamError,
    ParamService, ParamTiming, PARAM_PCK_PATH,
};
use control_station_mavftp::{crc32, Frame, Opcode};
use control_station_mavlink::{
    dialect::{
        MavAutopilot, MavModeFlag, MavState, MavType, FILE_TRANSFER_PROTOCOL_DATA, HEARTBEAT_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion, Parser,
};
use tokio::net::UdpSocket;

type Handler = Box<dyn FnMut(&MavMessage) -> Vec<MavMessage> + Send>;

struct Vehicle {
    received: Arc<Mutex<Vec<MavMessage>>>,
}

impl Vehicle {
    /// Heartbeats keep the vehicle alive; `handler` answers every message from the GCS.
    async fn start(gcs_port: u16, mut handler: Handler) -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let target = format!("127.0.0.1:{gcs_port}");
        let sequence = Arc::new(AtomicU8::new(0));
        let frame = {
            let sequence = sequence.clone();
            move |message: &MavMessage| {
                encode(
                    MavHeader {
                        system_id: 1,
                        component_id: 1,
                        sequence: sequence.fetch_add(1, Ordering::Relaxed),
                    },
                    message,
                    MavlinkVersion::V2,
                )
            }
        };
        {
            let (socket, frame, target) = (socket.clone(), frame.clone(), target.clone());
            tokio::spawn(async move {
                loop {
                    let heartbeat = MavMessage::HEARTBEAT(HEARTBEAT_DATA {
                        custom_mode: 0,
                        mavtype: MavType::MAV_TYPE_QUADROTOR,
                        autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
                        base_mode: MavModeFlag::empty(),
                        system_status: MavState::MAV_STATE_STANDBY,
                        mavlink_version: 3,
                    });
                    socket.send_to(&frame(&heartbeat), &target).await.ok();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
        }
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = received.clone();
        tokio::spawn(async move {
            let mut parser = Parser::new();
            let mut buf = [0u8; 2048];
            while let Ok(n) = socket.recv(&mut buf).await {
                for packet in parser.push(&buf[..n]) {
                    log.lock().unwrap().push(packet.message.clone());
                    for reply in handler(&packet.message) {
                        socket.send_to(&frame(&reply), &target).await.ok();
                    }
                }
            }
        });
        Self { received }
    }

    fn count(&self, matches: impl Fn(&MavMessage) -> bool) -> usize {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter(|m| matches(m))
            .count()
    }
}

#[derive(Default)]
struct Fs {
    files: BTreeMap<String, Vec<u8>>,
    dirs: Vec<String>,
}

#[derive(Clone, Copy, Default)]
struct Quirks {
    /// NAK ListDirectoryWithTime like old firmware.
    no_timed_listing: bool,
    /// Skip the third chunk of the first burst.
    drop_a_burst_chunk: bool,
    /// NAK BurstReadFile like firmware without it.
    no_burst: bool,
    /// Store a corrupted copy of uploads.
    corrupt_writes: bool,
}

struct FakeFtp {
    fs: Arc<Mutex<Fs>>,
    quirks: Quirks,
    dropped: bool,
    open: BTreeMap<u8, String>,
    next_session: u8,
}

fn reply(request: &Frame, ack: bool) -> Frame {
    let mut f = Frame::request(
        request.seq.wrapping_add(1),
        if ack { Opcode::Ack } else { Opcode::Nak },
    );
    f.req_opcode = request.opcode;
    f.session = request.session;
    f
}

fn wrap(frame: Frame) -> MavMessage {
    MavMessage::FILE_TRANSFER_PROTOCOL(FILE_TRANSFER_PROTOCOL_DATA {
        target_network: 0,
        target_system: 255,
        target_component: 190,
        payload: frame.encode().unwrap(),
    })
}

fn nak(request: &Frame, code: u8) -> Vec<MavMessage> {
    vec![wrap(reply(request, false).with_data(vec![code]))]
}

impl FakeFtp {
    fn new(fs: Arc<Mutex<Fs>>, quirks: Quirks) -> Self {
        Self {
            fs,
            quirks,
            dropped: false,
            open: BTreeMap::new(),
            next_session: 1,
        }
    }

    fn handle(&mut self, message: &MavMessage) -> Vec<MavMessage> {
        let MavMessage::FILE_TRANSFER_PROTOCOL(data) = message else {
            return vec![];
        };
        let request = Frame::decode(&data.payload).unwrap();
        let text = |d: &[u8]| {
            String::from_utf8_lossy(d)
                .trim_end_matches('\0')
                .to_string()
        };
        let path = text(&request.data);
        let mut fs = self.fs.lock().unwrap();
        match request.opcode {
            Opcode::ListDirectory | Opcode::ListDirectoryWithTime => {
                let timed = request.opcode == Opcode::ListDirectoryWithTime;
                if timed && self.quirks.no_timed_listing {
                    return nak(&request, 7);
                }
                let prefix = if path == "/" {
                    "/".to_string()
                } else {
                    format!("{path}/")
                };
                if path != "/" && !fs.dirs.contains(&path) {
                    return nak(&request, 10);
                }
                let mut records: Vec<Vec<u8>> = Vec::new();
                for dir in fs.dirs.iter().filter(|d| {
                    d.starts_with(&prefix) && !d[prefix.len()..].contains('/') && **d != path
                }) {
                    let name = &dir[prefix.len()..];
                    records.push(if timed {
                        format!("D{name}\t0\t1700000000\0").into_bytes()
                    } else {
                        format!("D{name}\0").into_bytes()
                    });
                }
                for (file, bytes) in fs
                    .files
                    .iter()
                    .filter(|(f, _)| f.starts_with(&prefix) && !f[prefix.len()..].contains('/'))
                {
                    let name = &file[prefix.len()..];
                    records.push(if timed {
                        format!("F{name}\t{}\t1700000001\0", bytes.len()).into_bytes()
                    } else {
                        format!("F{name}\t{}\0", bytes.len()).into_bytes()
                    });
                }
                let skip = request.offset as usize;
                if skip >= records.len() {
                    return nak(&request, 6);
                }
                let mut body = Vec::new();
                for record in &records[skip..] {
                    if body.len() + record.len() > 239 {
                        break;
                    }
                    body.extend(record);
                }
                vec![wrap(reply(&request, true).with_data(body))]
            }
            Opcode::OpenFileRO => match fs.files.get(&path) {
                None => nak(&request, 10),
                Some(bytes) => {
                    let session = self.next_session;
                    self.next_session += 1;
                    self.open.insert(session, path);
                    let mut r = reply(&request, true).with_data((bytes.len() as u32).to_le_bytes());
                    r.session = session;
                    vec![wrap(r)]
                }
            },
            Opcode::BurstReadFile => {
                if self.quirks.no_burst {
                    return nak(&request, 7);
                }
                let Some(file) = self
                    .open
                    .get(&request.session)
                    .and_then(|p| fs.files.get(p))
                else {
                    return nak(&request, 4);
                };
                let step = usize::from(request.size);
                let mut out = Vec::new();
                let mut offset = request.offset as usize;
                let mut index = 0;
                while offset < file.len() && index < 6 {
                    let end = (offset + step).min(file.len());
                    let mut r = reply(&request, true).with_data(file[offset..end].to_vec());
                    r.offset = offset as u32;
                    r.burst_complete = end == file.len() || index == 5;
                    let drop = self.quirks.drop_a_burst_chunk && !self.dropped && index == 2;
                    if drop {
                        self.dropped = true;
                    } else {
                        out.push(wrap(r));
                    }
                    offset = end;
                    index += 1;
                }
                out
            }
            Opcode::ReadFile => {
                let Some(file) = self
                    .open
                    .get(&request.session)
                    .and_then(|p| fs.files.get(p))
                else {
                    return nak(&request, 4);
                };
                let start = (request.offset as usize).min(file.len());
                if start == file.len() {
                    return nak(&request, 6);
                }
                let end = (start + usize::from(request.size)).min(file.len());
                let mut r = reply(&request, true).with_data(file[start..end].to_vec());
                r.offset = request.offset;
                vec![wrap(r)]
            }
            Opcode::CreateFile => {
                if !fs
                    .dirs
                    .contains(&path.rsplit_once('/').map_or("/".into(), |(d, _)| {
                        if d.is_empty() {
                            "/".into()
                        } else {
                            d.to_string()
                        }
                    }))
                {
                    return nak(&request, 10);
                }
                fs.files.insert(path.clone(), Vec::new());
                let session = self.next_session;
                self.next_session += 1;
                self.open.insert(session, path);
                let mut r = reply(&request, true);
                r.session = session;
                vec![wrap(r)]
            }
            Opcode::WriteFile => {
                let Some(path) = self.open.get(&request.session).cloned() else {
                    return nak(&request, 4);
                };
                let file = fs.files.get_mut(&path).unwrap();
                let at = request.offset as usize;
                if file.len() < at + request.data.len() {
                    file.resize(at + request.data.len(), 0);
                }
                file[at..at + request.data.len()].copy_from_slice(&request.data);
                if self.quirks.corrupt_writes && at == 0 {
                    file[0] ^= 0xFF;
                }
                vec![wrap(reply(&request, true))]
            }
            Opcode::TerminateSession => {
                self.open.remove(&request.session);
                vec![wrap(reply(&request, true))]
            }
            Opcode::ResetSessions => {
                self.open.clear();
                vec![wrap(reply(&request, true))]
            }
            Opcode::CalcFileCRC32 => match fs.files.get(&path) {
                None => nak(&request, 10),
                Some(bytes) => vec![wrap(
                    reply(&request, true).with_data(crc32(0, bytes).to_le_bytes()),
                )],
            },
            Opcode::RemoveFile => {
                if fs.files.remove(&path).is_some() {
                    vec![wrap(reply(&request, true))]
                } else {
                    nak(&request, 10)
                }
            }
            Opcode::CreateDirectory => {
                if fs.dirs.contains(&path) {
                    return nak(&request, 8);
                }
                fs.dirs.push(path);
                vec![wrap(reply(&request, true))]
            }
            Opcode::RemoveDirectory => {
                let prefix = format!("{path}/");
                let busy = fs.files.keys().any(|f| f.starts_with(&prefix))
                    || fs.dirs.iter().any(|d| d.starts_with(&prefix));
                if busy {
                    return vec![wrap(reply(&request, false).with_data(vec![2, 39]))];
                }
                fs.dirs.retain(|d| *d != path);
                vec![wrap(reply(&request, true))]
            }
            Opcode::Rename => {
                let mut parts = request.data.split(|&b| b == 0);
                let from = text(parts.next().unwrap());
                let to = text(parts.next().unwrap());
                match fs.files.remove(&from) {
                    Some(bytes) => {
                        fs.files.insert(to, bytes);
                        vec![wrap(reply(&request, true))]
                    }
                    None => nak(&request, 10),
                }
            }
            _ => nak(&request, 7),
        }
    }
}

fn quick() -> FtpTiming {
    FtpTiming {
        request_timeout: Duration::from_millis(200),
        tries: 3,
        burst_idle: Duration::from_millis(250),
        burst_tries: 3,
        read_chunk: 100,
        write_chunk: 60,
    }
}

fn sample_fs() -> Arc<Mutex<Fs>> {
    let mut fs = Fs {
        dirs: ["/APM", "/APM/scripts", "/APM/logs"]
            .map(String::from)
            .to_vec(),
        ..Fs::default()
    };
    fs.files
        .insert("/APM/scripts/hello.lua".into(), b"print('hi')\n".to_vec());
    fs.files.insert(
        "/APM/big.bin".into(),
        (0..2000u32).map(|i| (i * 7 % 251) as u8).collect(),
    );
    Arc::new(Mutex::new(fs))
}

async fn rig(quirks: Quirks) -> (FtpService, Arc<Mutex<Fs>>, Vehicle) {
    let (ftp, fs, vehicle, _) = rig_with_link(quirks).await;
    (ftp, fs, vehicle)
}

async fn rig_with_link(quirks: Quirks) -> (FtpService, Arc<Mutex<Fs>>, Vehicle, LinkManager) {
    let fs = sample_fs();
    let mut fake = FakeFtp::new(fs.clone(), quirks);
    let link = LinkManager::new();
    link.connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let vehicle = Vehicle::start(
        link.local_port().unwrap(),
        Box::new(move |m| fake.handle(m)),
    )
    .await;
    let started = tokio::time::Instant::now();
    while !link.telemetry().connected {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "no vehicle heartbeat"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    (
        FtpService::new(link.clone()).with_timing(quick()),
        fs,
        vehicle,
        link,
    )
}

#[tokio::test]
async fn lists_a_directory_with_times() {
    let (ftp, _, _v) = rig(Quirks::default()).await;
    let root = ftp.list("/APM").await.unwrap();
    let names: Vec<_> = root.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
    assert_eq!(
        names,
        [("scripts", true), ("logs", true), ("big.bin", false)]
    );
    assert_eq!(root[2].size, 2000);
    assert_eq!(root[2].modified, Some(1_700_000_001));
    let scripts = ftp.list("/APM/scripts/").await.unwrap();
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].name, "hello.lua");
}

#[tokio::test]
async fn lists_across_several_pages() {
    let (ftp, fs, _v) = rig(Quirks::default()).await;
    for i in 0..30 {
        fs.lock()
            .unwrap()
            .files
            .insert(format!("/APM/logs/file_number_{i:02}.bin"), vec![0; i]);
    }
    let entries = ftp.list("/APM/logs").await.unwrap();
    assert_eq!(entries.len(), 30);
    assert_eq!(entries[29].name, "file_number_29.bin");
}

#[tokio::test]
async fn falls_back_to_plain_listing_on_old_firmware() {
    let (ftp, _, v) = rig(Quirks {
        no_timed_listing: true,
        ..Quirks::default()
    })
    .await;
    let root = ftp.list("/APM").await.unwrap();
    assert_eq!(root.len(), 3);
    assert_eq!(root[2].modified, None);
    // the second listing goes straight to the plain opcode
    ftp.list("/APM").await.unwrap();
    let timed = v.count(|m| {
        matches!(m, MavMessage::FILE_TRANSFER_PROTOCOL(d)
        if Frame::decode(&d.payload).unwrap().opcode == Opcode::ListDirectoryWithTime)
    });
    assert_eq!(timed, 1);
}

#[tokio::test]
async fn missing_directory_is_not_found() {
    let (ftp, _, _v) = rig(Quirks::default()).await;
    match ftp.list("/nope").await {
        Err(FtpServiceError::Refused {
            error: FtpError::FileNotFound,
            ..
        }) => {}
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn reads_a_file_with_burst() {
    let (ftp, fs, _v) = rig(Quirks::default()).await;
    let bytes = ftp.read("/APM/big.bin").await.unwrap();
    assert_eq!(bytes, fs.lock().unwrap().files["/APM/big.bin"]);
    assert_eq!(
        ftp.read("/APM/scripts/hello.lua").await.unwrap(),
        b"print('hi')\n"
    );
}

#[tokio::test]
async fn burst_recovers_from_a_lost_chunk() {
    let (ftp, fs, _v) = rig(Quirks {
        drop_a_burst_chunk: true,
        ..Quirks::default()
    })
    .await;
    let bytes = ftp.read("/APM/big.bin").await.unwrap();
    assert_eq!(bytes, fs.lock().unwrap().files["/APM/big.bin"]);
}

#[tokio::test]
async fn reads_without_burst_support() {
    let (ftp, fs, _v) = rig(Quirks {
        no_burst: true,
        ..Quirks::default()
    })
    .await;
    let bytes = ftp.read("/APM/big.bin").await.unwrap();
    assert_eq!(bytes, fs.lock().unwrap().files["/APM/big.bin"]);
}

#[tokio::test]
async fn reading_a_missing_file_fails_cleanly() {
    let (ftp, _, _v) = rig(Quirks::default()).await;
    assert!(matches!(
        ftp.read("/APM/nothing.txt").await,
        Err(FtpServiceError::Refused {
            error: FtpError::FileNotFound,
            ..
        })
    ));
}

#[tokio::test]
async fn uploads_with_crc_verification() {
    let (ftp, fs, _v) = rig(Quirks::default()).await;
    let script: Vec<u8> = (0..700u32).map(|i| b'a' + (i % 26) as u8).collect();
    ftp.write("/APM/scripts/new.lua", &script, true)
        .await
        .unwrap();
    assert_eq!(fs.lock().unwrap().files["/APM/scripts/new.lua"], script);
    // an empty file is created too
    ftp.write("/APM/scripts/empty.lua", &[], true)
        .await
        .unwrap();
    assert!(fs.lock().unwrap().files["/APM/scripts/empty.lua"].is_empty());
}

#[tokio::test]
async fn detects_a_corrupted_upload() {
    let (ftp, _, _v) = rig(Quirks {
        corrupt_writes: true,
        ..Quirks::default()
    })
    .await;
    match ftp.write("/APM/scripts/bad.lua", b"print(1)", true).await {
        Err(FtpServiceError::CrcMismatch { local, remote }) => assert_ne!(local, remote),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn upload_into_a_missing_directory_is_refused() {
    let (ftp, _, _v) = rig(Quirks::default()).await;
    assert!(matches!(
        ftp.write("/APM/missing/x.lua", b"x", false).await,
        Err(FtpServiceError::Refused {
            error: FtpError::FileNotFound,
            ..
        })
    ));
}

#[tokio::test]
async fn file_operations() {
    let (ftp, fs, _v) = rig(Quirks::default()).await;
    ftp.mkdir("/APM/new").await.unwrap();
    assert!(matches!(
        ftp.mkdir("/APM/new").await,
        Err(FtpServiceError::Refused {
            error: FtpError::FileExists,
            ..
        })
    ));
    ftp.rename("/APM/scripts/hello.lua", "/APM/new/moved.lua")
        .await
        .unwrap();
    assert!(fs.lock().unwrap().files.contains_key("/APM/new/moved.lua"));
    // non-empty directory: the vehicle answers with an errno
    assert!(matches!(
        ftp.rmdir("/APM/new").await,
        Err(FtpServiceError::Refused {
            error: FtpError::FailErrno(39),
            ..
        })
    ));
    ftp.remove("/APM/new/moved.lua").await.unwrap();
    ftp.rmdir("/APM/new").await.unwrap();
    assert!(!fs.lock().unwrap().dirs.contains(&"/APM/new".to_string()));
    let crc = ftp.crc32("/APM/big.bin").await.unwrap();
    assert_eq!(crc, crc32(0, &fs.lock().unwrap().files["/APM/big.bin"]));
}

#[tokio::test]
async fn silent_vehicle_times_out_and_bad_paths_are_rejected() {
    let (ftp, _, _v) = rig(Quirks::default()).await;
    assert!(matches!(
        ftp.list("").await,
        Err(FtpServiceError::BadPath(_))
    ));
    assert!(matches!(
        ftp.remove(&"x".repeat(300)).await,
        Err(FtpServiceError::BadPath(_))
    ));
    // a vehicle that never answers
    let link = LinkManager::new();
    link.connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let silent = Vehicle::start(link.local_port().unwrap(), Box::new(|_| vec![])).await;
    while !link.telemetry().connected {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let ftp = FtpService::new(link).with_timing(quick());
    assert!(matches!(
        ftp.list("/").await,
        Err(FtpServiceError::Timeout("list"))
    ));
    assert!(silent.count(|m| matches!(m, MavMessage::FILE_TRANSFER_PROTOCOL(_))) >= 3);
}

#[tokio::test]
async fn reports_progress_and_can_be_cancelled() {
    let (ftp, _, _v) = rig(Quirks::default()).await;
    let mut progress = ftp.subscribe_progress();
    ftp.read("/APM/big.bin").await.unwrap();
    let mut seen = Vec::new();
    while let Ok(p) = progress.try_recv() {
        seen.push(p);
    }
    assert_eq!(seen.first().unwrap().op, "read");
    assert!(!seen.first().unwrap().finished);
    assert!(seen.last().unwrap().finished && seen.last().unwrap().error.is_none());
    assert!(seen.iter().any(|p| p.total == 2000 && p.done > 0));

    let big = vec![7u8; 1_000_000];
    let writer = ftp.clone();
    let task = tokio::spawn(async move { writer.write("/APM/scripts/big.lua", &big, false).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    ftp.cancel();
    assert!(matches!(
        task.await.unwrap(),
        Err(FtpServiceError::Cancelled)
    ));
}

#[tokio::test]
async fn second_operation_while_busy_is_rejected() {
    let (ftp, _, _v) = rig(Quirks::default()).await;
    let other = ftp.clone();
    let task = tokio::spawn(async move {
        other
            .write("/APM/scripts/slow.lua", &vec![1u8; 1_000_000], false)
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(matches!(ftp.list("/").await, Err(FtpServiceError::Busy)));
    ftp.cancel();
    let _ = task.await;
}

fn pck(entries: &[(&str, f32)]) -> Vec<u8> {
    let mut out = 0x671bu16.to_le_bytes().to_vec();
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    for (name, value) in entries {
        out.extend([4, ((name.len() - 1) as u8) << 4]);
        out.extend(name.as_bytes());
        out.extend(value.to_le_bytes());
    }
    out
}

fn quick_params() -> ParamTiming {
    ParamTiming {
        idle_timeout: Duration::from_millis(150),
        read_round: Duration::from_millis(20),
        read_batch: 5,
        max_idle_rounds: 1,
        echo_timeout: Duration::from_millis(100),
        set_attempts: 1,
    }
}

#[tokio::test]
async fn fast_parameter_download_over_mavftp() {
    let (ftp, fs, _v, link) = rig_with_link(Quirks::default()).await;
    let entries: Vec<(String, f32)> = (0..60)
        .map(|i| (format!("PARAM_{i:02}"), i as f32))
        .collect();
    let refs: Vec<(&str, f32)> = entries.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    fs.lock()
        .unwrap()
        .files
        .insert(PARAM_PCK_PATH.into(), pck(&refs));
    let params = ParamService::new(link)
        .with_timing(quick_params())
        .with_ftp(ftp);
    assert_eq!(params.refresh().await.unwrap(), 60);
    let list = params.list();
    assert_eq!(list.len(), 60);
    assert_eq!(list[7].name, "PARAM_07");
    assert_eq!(list[7].value, 7.0);
}

#[tokio::test]
async fn parameters_fall_back_to_mavlink_and_do_not_retry_ftp() {
    // the fake has no param.pck and ignores PARAM_REQUEST_LIST, so the fallback ends in an error
    // after the FTP attempt was refused once.
    let (ftp, _fs, v, link) = rig_with_link(Quirks::default()).await;
    let params = ParamService::new(link)
        .with_timing(quick_params())
        .with_ftp(ftp);
    assert!(matches!(
        params.refresh().await,
        Err(ParamError::NothingReceived)
    ));
    let ftp_requests =
        |v: &Vehicle| v.count(|m| matches!(m, MavMessage::FILE_TRANSFER_PROTOCOL(_)));
    let after_first = ftp_requests(&v);
    assert!(after_first >= 1);
    assert!(matches!(
        params.refresh().await,
        Err(ParamError::NothingReceived)
    ));
    assert_eq!(ftp_requests(&v), after_first);
}

#[tokio::test]
async fn replay_serves_files() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../exemplos/Teste 02 - tunning.BIN"
    );
    if !std::path::Path::new(path).exists() {
        eprintln!("skipping: sample log not present");
        return;
    }
    let link = LinkManager::new();
    link.connect(ConnectionConfig::Replay {
        path: path.into(),
        speed: Some(50.0),
    })
    .await
    .unwrap();
    let started = tokio::time::Instant::now();
    while !link.telemetry().connected {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no replay vehicle"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let ftp = FtpService::new(link);
    let root = ftp.list("/APM").await.unwrap();
    assert!(root.iter().any(|e| e.name == "scripts" && e.is_dir));
    let script = b"print('replay')\n".repeat(40);
    ftp.write("/APM/scripts/t.lua", &script, true)
        .await
        .unwrap();
    assert_eq!(ftp.read("/APM/scripts/t.lua").await.unwrap(), script);
    ftp.remove("/APM/scripts/t.lua").await.unwrap();
}
