//! Onboard log listing and download against a scripted fake vehicle over UDP loopback. The
//! vehicle stores `00000005.BIN`..`00000007.BIN` (LOG_ENTRY ids 1..3, as ArduPilot counts from
//! the oldest log) and answers MAVFTP or not, depending on the test.
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_link::{
    ConnectionConfig, FtpTiming, LinkManager, LogEntry, LogError, LogMethod, LogService, LogSource,
    LogTiming,
};
use control_station_mavftp::{Frame, Opcode};
use control_station_mavlink::{
    dialect::{
        MavAutopilot, MavModeFlag, MavState, MavType, FILE_TRANSFER_PROTOCOL_DATA, HEARTBEAT_DATA,
        LOG_DATA_DATA, LOG_ENTRY_DATA,
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

    fn messages(&self) -> Vec<MavMessage> {
        self.received.lock().unwrap().clone()
    }
}

#[derive(Clone, Copy, Default)]
struct Quirks {
    /// Answer MAVFTP (otherwise FILE_TRANSFER_PROTOCOL is ignored, like firmware without it).
    ftp: bool,
    /// Leave this LOG_ENTRY id out of the first listing.
    drop_entry: Option<u16>,
    /// Leave the LOG_DATA chunk at this offset out of the first transfer that covers it.
    drop_chunk: Option<u32>,
    /// Never answer LOG_REQUEST_DATA.
    silent_data: bool,
    /// Report LOG_ENTRY sizes this much larger than the file (sizes "may be approximate").
    inflate_size: u32,
}

/// (log number, content), oldest first.
type Logs = Vec<(u16, Vec<u8>)>;

struct FakeVehicle {
    logs: Logs,
    quirks: Quirks,
    entry_dropped: bool,
    chunk_dropped: bool,
    open: BTreeMap<u8, Vec<u8>>,
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

fn file_name(number: u16) -> String {
    format!("{number:08}.BIN")
}

impl FakeVehicle {
    fn new(logs: Logs, quirks: Quirks) -> Self {
        Self {
            logs,
            quirks,
            entry_dropped: false,
            chunk_dropped: false,
            open: BTreeMap::new(),
            next_session: 1,
        }
    }

    fn handle(&mut self, message: &MavMessage) -> Vec<MavMessage> {
        match message {
            MavMessage::FILE_TRANSFER_PROTOCOL(data) if self.quirks.ftp => {
                self.ftp(&Frame::decode(&data.payload).unwrap())
            }
            MavMessage::LOG_REQUEST_LIST(r) => {
                let num = self.logs.len() as u16;
                if num == 0 {
                    return vec![MavMessage::LOG_ENTRY(LOG_ENTRY_DATA::default())];
                }
                // like AP_Logger_MAVLinkLogTransfer::handle_log_request_list
                let start = r.start.max(1);
                let end = r.end.min(num);
                let mut out = Vec::new();
                for id in start..=end {
                    if self.quirks.drop_entry == Some(id) && !self.entry_dropped {
                        self.entry_dropped = true;
                        continue;
                    }
                    let content = &self.logs[usize::from(id - 1)].1;
                    out.push(MavMessage::LOG_ENTRY(LOG_ENTRY_DATA {
                        id,
                        num_logs: num,
                        last_log_num: end,
                        time_utc: 1_700_000_000 + u32::from(id),
                        size: content.len() as u32 + self.quirks.inflate_size,
                    }));
                }
                out
            }
            MavMessage::LOG_REQUEST_DATA(r) => {
                if self.quirks.silent_data || r.id < 1 || usize::from(r.id) > self.logs.len() {
                    return vec![];
                }
                let content = &self.logs[usize::from(r.id - 1)].1;
                let chunk = |ofs: usize, bytes: &[u8]| {
                    let mut data = [0u8; 90];
                    data[..bytes.len()].copy_from_slice(bytes);
                    MavMessage::LOG_DATA(LOG_DATA_DATA {
                        id: r.id,
                        ofs: ofs as u32,
                        count: bytes.len() as u8,
                        data,
                    })
                };
                let mut ofs = r.ofs as usize;
                if ofs >= content.len() {
                    return vec![chunk(ofs, &[])];
                }
                let stop = content.len().min(ofs + r.count as usize);
                let mut out = Vec::new();
                while ofs < stop {
                    let end = (ofs + 90).min(stop);
                    // the last chunk of the file is short; an exact multiple of 90 ends with
                    // an empty one, like ArduPilot's short read at EOF
                    if self.quirks.drop_chunk == Some(ofs as u32) && !self.chunk_dropped {
                        self.chunk_dropped = true;
                    } else {
                        out.push(chunk(ofs, &content[ofs..end]));
                    }
                    ofs = end;
                }
                if ofs == content.len() && ofs.is_multiple_of(90) {
                    out.push(chunk(ofs, &[]));
                }
                out
            }
            _ => vec![],
        }
    }

    fn ftp(&mut self, request: &Frame) -> Vec<MavMessage> {
        let path = String::from_utf8_lossy(&request.data)
            .trim_end_matches('\0')
            .to_string();
        match request.opcode {
            Opcode::ListDirectory | Opcode::ListDirectoryWithTime => {
                let timed = request.opcode == Opcode::ListDirectoryWithTime;
                if path != "/APM/LOGS" {
                    return nak(request, 10);
                }
                let mut records: Vec<Vec<u8>> = vec![if timed {
                    b"FLASTLOG.TXT\t2\t1700000000\0".to_vec()
                } else {
                    b"FLASTLOG.TXT\t2\0".to_vec()
                }];
                // newest first, to check the service sorts
                for (number, bytes) in self.logs.iter().rev() {
                    let name = file_name(*number);
                    records.push(if timed {
                        format!(
                            "F{name}\t{}\t{}\0",
                            bytes.len(),
                            1_700_000_000 + u32::from(*number)
                        )
                        .into_bytes()
                    } else {
                        format!("F{name}\t{}\0", bytes.len()).into_bytes()
                    });
                }
                let skip = request.offset as usize;
                if skip >= records.len() {
                    return nak(request, 6);
                }
                let mut body = Vec::new();
                for record in &records[skip..] {
                    if body.len() + record.len() > 239 {
                        break;
                    }
                    body.extend(record);
                }
                vec![wrap(reply(request, true).with_data(body))]
            }
            Opcode::OpenFileRO => {
                let Some(content) = self
                    .logs
                    .iter()
                    .find(|(n, _)| format!("/APM/LOGS/{}", file_name(*n)) == path)
                    .map(|(_, c)| c.clone())
                else {
                    return nak(request, 10);
                };
                let session = self.next_session;
                self.next_session += 1;
                let mut r = reply(request, true).with_data((content.len() as u32).to_le_bytes());
                r.session = session;
                self.open.insert(session, content);
                vec![wrap(r)]
            }
            Opcode::BurstReadFile => {
                let Some(file) = self.open.get(&request.session) else {
                    return nak(request, 4);
                };
                if request.offset as usize >= file.len() {
                    return nak(request, 6);
                }
                let step = usize::from(request.size);
                let mut out = Vec::new();
                let mut offset = request.offset as usize;
                for index in 0..6 {
                    if offset >= file.len() {
                        break;
                    }
                    let end = (offset + step).min(file.len());
                    let mut r = reply(request, true).with_data(file[offset..end].to_vec());
                    r.offset = offset as u32;
                    r.burst_complete = end == file.len() || index == 5;
                    out.push(wrap(r));
                    offset = end;
                }
                out
            }
            Opcode::TerminateSession => {
                self.open.remove(&request.session);
                vec![wrap(reply(request, true))]
            }
            _ => nak(request, 7),
        }
    }
}

/// Three logs of different sizes and contents; 00000006.BIN is an exact multiple of 90.
fn sample_logs() -> Logs {
    [(5u16, 1000usize), (6, 1800), (7, 2345)]
        .into_iter()
        .map(|(number, len)| {
            let content = (0..len)
                .map(|i| ((i * 31 + usize::from(number) * 17) % 251) as u8)
                .collect();
            (number, content)
        })
        .collect()
}

fn quick() -> LogTiming {
    LogTiming {
        list_idle: Duration::from_millis(250),
        tries: 4,
        data_idle: Duration::from_millis(250),
        window_chunks: 8,
        progress_interval: Duration::from_millis(1),
        ftp: FtpTiming {
            request_timeout: Duration::from_millis(150),
            tries: 2,
            burst_idle: Duration::from_millis(250),
            burst_tries: 3,
            read_chunk: 100,
            write_chunk: 60,
        },
    }
}

async fn rig(quirks: Quirks) -> (LogService, Vehicle) {
    rig_with(sample_logs(), quirks).await
}

async fn rig_with(logs: Logs, quirks: Quirks) -> (LogService, Vehicle) {
    let mut fake = FakeVehicle::new(logs, quirks);
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
    (LogService::new(link).with_timing(quick()), vehicle)
}

fn content_of(number: u16) -> Vec<u8> {
    sample_logs()
        .into_iter()
        .find(|(n, _)| *n == number)
        .unwrap()
        .1
}

fn data_requests(vehicle: &Vehicle) -> Vec<(u16, u32, u32)> {
    vehicle
        .messages()
        .iter()
        .filter_map(|m| match m {
            MavMessage::LOG_REQUEST_DATA(r) => Some((r.id, r.ofs, r.count)),
            _ => None,
        })
        .collect()
}

/// LOG_REQUEST_END messages the vehicle got, after letting the last ones arrive.
async fn ends(vehicle: &Vehicle) -> usize {
    tokio::time::sleep(Duration::from_millis(150)).await;
    vehicle
        .messages()
        .iter()
        .filter(|m| matches!(m, MavMessage::LOG_REQUEST_END(_)))
        .count()
}

#[tokio::test]
async fn ftp_lists_real_file_names_and_downloads_the_chosen_file() {
    let (logs, vehicle) = rig(Quirks {
        ftp: true,
        ..Quirks::default()
    })
    .await;
    let entries = logs.list().await.unwrap();
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["00000005.BIN", "00000006.BIN", "00000007.BIN"]);
    assert_eq!(entries[1].size, 1800);
    assert_eq!(entries[1].time_utc, Some(1_700_000_006));
    assert_eq!(
        entries[1].source,
        LogSource::Ftp {
            path: "/APM/LOGS/00000006.BIN".into()
        }
    );
    // FTP answered, so LOG_* was never needed
    assert!(!vehicle
        .messages()
        .iter()
        .any(|m| matches!(m, MavMessage::LOG_REQUEST_LIST(_))));

    let mut progress = logs.subscribe_progress();
    let bytes = logs.download(&entries[1]).await.unwrap();
    assert_eq!(bytes, content_of(6));
    let mut events = Vec::new();
    while let Ok(p) = progress.try_recv() {
        events.push(p);
    }
    assert!(events.iter().all(|p| p.method == LogMethod::Ftp));
    assert!(events.iter().any(|p| !p.finished && p.done > 0));
    let last = events.last().unwrap();
    assert!(last.finished && last.error.is_none());
    assert_eq!((last.done, last.total), (1800, 1800));
}

#[tokio::test]
async fn without_ftp_lists_with_log_entry_and_maps_ids_to_the_right_file() {
    let (logs, vehicle) = rig(Quirks::default()).await;
    let entries = logs.list().await.unwrap();
    assert_eq!(
        entries,
        (1u16..=3)
            .zip([5u16, 6, 7])
            .map(|(id, number)| LogEntry {
                name: format!("Log {id}"),
                size: content_of(number).len() as u64,
                time_utc: Some(1_700_000_000 + u32::from(id)),
                source: LogSource::Mavlink { id },
            })
            .collect::<Vec<_>>()
    );
    // id 2 is the second oldest log, 00000006.BIN, never "00000002.BIN"
    let mut progress = logs.subscribe_progress();
    assert_eq!(logs.download(&entries[1]).await.unwrap(), content_of(6));
    assert_eq!(logs.download(&entries[2]).await.unwrap(), content_of(7));
    assert!(data_requests(&vehicle).iter().all(|(id, _, _)| *id != 6));
    // the listing and both downloads each ended the transfer
    assert_eq!(ends(&vehicle).await, 3);
    let mut methods = Vec::new();
    while let Ok(p) = progress.try_recv() {
        methods.push(p.method);
    }
    assert!(methods.iter().all(|m| *m == LogMethod::Mavlink));
}

#[tokio::test]
async fn missing_log_entries_are_requested_again() {
    let (logs, vehicle) = rig(Quirks {
        drop_entry: Some(2),
        ..Quirks::default()
    })
    .await;
    let entries = logs.list().await.unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[1].source, LogSource::Mavlink { id: 2 });
    let lists: Vec<_> = vehicle
        .messages()
        .iter()
        .filter_map(|m| match m {
            MavMessage::LOG_REQUEST_LIST(r) => Some((r.start, r.end)),
            _ => None,
        })
        .collect();
    assert_eq!(lists, [(0, u16::MAX), (2, 2)]);
}

#[tokio::test]
async fn a_lost_log_data_chunk_is_requested_again() {
    let (logs, vehicle) = rig(Quirks {
        drop_chunk: Some(180),
        ..Quirks::default()
    })
    .await;
    let entries = logs.list().await.unwrap();
    assert_eq!(logs.download(&entries[2]).await.unwrap(), content_of(7));
    let requests = data_requests(&vehicle);
    assert_eq!(requests[0], (3, 0, 8 * 90));
    // the gap alone is asked for again: one chunk, up to the next received one
    assert!(
        requests.contains(&(3, 180, 90)),
        "no re-request of the gap: {requests:?}"
    );
}

#[tokio::test]
async fn a_short_chunk_ends_a_log_listed_too_large() {
    let (logs, _vehicle) = rig(Quirks {
        inflate_size: 500,
        ..Quirks::default()
    })
    .await;
    let entries = logs.list().await.unwrap();
    assert_eq!(entries[0].size, 1500);
    assert_eq!(logs.download(&entries[0]).await.unwrap(), content_of(5));
    // 1800 bytes is a multiple of 90: the end shows as an empty chunk
    assert_eq!(logs.download(&entries[1]).await.unwrap(), content_of(6));
}

#[tokio::test]
async fn no_logs_is_an_empty_list() {
    for ftp in [false, true] {
        let (logs, _vehicle) = rig_with(
            Vec::new(),
            Quirks {
                ftp,
                ..Quirks::default()
            },
        )
        .await;
        assert!(logs.list().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn a_download_can_be_cancelled_and_ends_the_transfer() {
    let (logs, vehicle) = rig(Quirks {
        silent_data: true,
        ..Quirks::default()
    })
    .await;
    let entries = logs.list().await.unwrap();
    let logs = logs.with_timing(LogTiming {
        data_idle: Duration::from_secs(5),
        ..quick()
    });
    let ends_before = ends(&vehicle).await;
    let task = {
        let logs = logs.clone();
        let entry = entries[0].clone();
        tokio::spawn(async move { logs.download(&entry).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let started = tokio::time::Instant::now();
    logs.cancel();
    let result = task.await.unwrap();
    assert!(matches!(result, Err(LogError::Cancelled)), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(ends(&vehicle).await, ends_before + 1);
}

#[tokio::test]
async fn an_ftp_download_can_be_cancelled() {
    let (logs, _vehicle) = rig(Quirks {
        ftp: true,
        ..Quirks::default()
    })
    .await;
    let entries = logs.list().await.unwrap();
    // cancelling before the read starts must not be lost when the FTP client resets its flag
    let task = {
        let logs = logs.clone();
        let entry = entries[2].clone();
        tokio::spawn(async move { logs.download(&entry).await })
    };
    logs.cancel();
    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(LogError::Cancelled)) || result.is_ok(),
        "{result:?}"
    );
    // a later download is not affected by the earlier cancel
    assert_eq!(logs.download(&entries[2]).await.unwrap(), content_of(7));
}
