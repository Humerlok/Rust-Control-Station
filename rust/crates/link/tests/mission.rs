//! Mission transfers against scripted fake vehicles over UDP loopback, plus a Replay round trip.
// The legacy MISSION_REQUEST message is part of what the service must answer.
#![allow(deprecated)]

use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_link::{
    ConnectionConfig, LinkManager, MissionError, MissionItem, MissionKind, MissionPhase,
    MissionService, MissionTiming,
};
use control_station_mavlink::{
    dialect::{
        MavAutopilot, MavCmd, MavFrame, MavMissionResult, MavMissionType, MavModeFlag, MavState,
        MavType, HEARTBEAT_DATA, MISSION_ACK_DATA, MISSION_COUNT_DATA, MISSION_ITEM_INT_DATA,
        MISSION_REQUEST_DATA, MISSION_REQUEST_INT_DATA,
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

fn quick() -> MissionTiming {
    MissionTiming {
        count_timeout: Duration::from_millis(150),
        download_count_tries: 3,
        upload_count_tries: 3,
        item_timeout: Duration::from_millis(250),
        item_tries: 3,
        clear_timeout: Duration::from_millis(200),
        clear_tries: 2,
    }
}

async fn rig(handler: Handler) -> (LinkManager, MissionService, Vehicle) {
    let link = LinkManager::new();
    link.connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let vehicle = Vehicle::start(link.local_port().unwrap(), handler).await;
    let started = tokio::time::Instant::now();
    while !link.telemetry().connected {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "no vehicle heartbeat"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let service = MissionService::new(link.clone()).with_timing(quick());
    (link, service, vehicle)
}

fn silent() -> Handler {
    Box::new(|_| Vec::new())
}

fn mission_items(n: usize) -> Vec<MissionItem> {
    (0..n)
        .map(|i| MissionItem {
            seq: i as u16,
            frame: if i == 0 { 0 } else { 3 },
            command: if i + 1 == n && n > 1 { 20 } else { 16 },
            param1: i as f64,
            param2: 0.0,
            param3: 0.0,
            param4: 0.0,
            x: -23.5 + i as f64 * 0.001,
            y: 46.25 + i as f64 * 0.002,
            z: 10.0 + i as f64,
            autocontinue: true,
        })
        .collect()
}

fn wire_item(i: &MissionItem, kind: MavMissionType) -> MISSION_ITEM_INT_DATA {
    MISSION_ITEM_INT_DATA {
        param1: i.param1 as f32,
        x: (i.x * 1e7).round() as i32,
        y: (i.y * 1e7).round() as i32,
        z: i.z as f32,
        seq: i.seq,
        command: if i.command == 20 {
            MavCmd::MAV_CMD_NAV_RETURN_TO_LAUNCH
        } else {
            MavCmd::MAV_CMD_NAV_WAYPOINT
        },
        frame: if i.frame == 0 {
            MavFrame::MAV_FRAME_GLOBAL
        } else {
            MavFrame::MAV_FRAME_GLOBAL_RELATIVE_ALT
        },
        autocontinue: 1,
        mission_type: kind,
        ..MISSION_ITEM_INT_DATA::default()
    }
}

fn close(a: &MissionItem, b: &MissionItem) -> bool {
    a.seq == b.seq
        && a.frame == b.frame
        && a.command == b.command
        && a.param1 == b.param1
        && (a.x - b.x).abs() < 1e-7
        && (a.y - b.y).abs() < 1e-7
        && a.z == b.z
        && a.autocontinue == b.autocontinue
}

fn ack(result: MavMissionResult, kind: MavMissionType) -> MavMessage {
    MavMessage::MISSION_ACK(MISSION_ACK_DATA {
        mavtype: result,
        mission_type: kind,
        ..MISSION_ACK_DATA::default()
    })
}

/// A vehicle holding `items` that serves downloads. `drop_item` loses the first answer for it.
fn download_vehicle(
    items: Vec<MissionItem>,
    kind: MavMissionType,
    drop_item: Option<u16>,
) -> Handler {
    let mut dropped = false;
    Box::new(move |message| match message {
        MavMessage::MISSION_REQUEST_LIST(_) => {
            vec![MavMessage::MISSION_COUNT(MISSION_COUNT_DATA {
                count: items.len() as u16,
                mission_type: kind,
                ..MISSION_COUNT_DATA::default()
            })]
        }
        MavMessage::MISSION_REQUEST_INT(r) => {
            if Some(r.seq) == drop_item && !dropped {
                dropped = true;
                return Vec::new();
            }
            items
                .get(usize::from(r.seq))
                .map(|i| vec![MavMessage::MISSION_ITEM_INT(wire_item(i, kind))])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Int,
    Legacy,
}

/// A vehicle that accepts uploads, requesting items with MISSION_REQUEST_INT or the legacy
/// MISSION_REQUEST, and keeps what it received.
fn upload_vehicle(
    style: Style,
    kind: MavMissionType,
    store: Arc<Mutex<Vec<MISSION_ITEM_INT_DATA>>>,
) -> Handler {
    let mut expected = 0usize;
    let request = move |seq: usize| match style {
        Style::Int => MavMessage::MISSION_REQUEST_INT(MISSION_REQUEST_INT_DATA {
            seq: seq as u16,
            mission_type: kind,
            ..MISSION_REQUEST_INT_DATA::default()
        }),
        Style::Legacy => MavMessage::MISSION_REQUEST(MISSION_REQUEST_DATA {
            seq: seq as u16,
            mission_type: kind,
            ..MISSION_REQUEST_DATA::default()
        }),
    };
    Box::new(move |message| match message {
        MavMessage::MISSION_COUNT(c) => {
            expected = usize::from(c.count);
            store.lock().unwrap().clear();
            if expected == 0 {
                vec![ack(MavMissionResult::MAV_MISSION_ACCEPTED, kind)]
            } else {
                vec![request(0)]
            }
        }
        MavMessage::MISSION_ITEM_INT(i) => {
            let mut store = store.lock().unwrap();
            if usize::from(i.seq) == store.len() {
                store.push(i.clone());
            }
            if store.len() == expected {
                vec![ack(MavMissionResult::MAV_MISSION_ACCEPTED, kind)]
            } else {
                vec![request(store.len())]
            }
        }
        _ => Vec::new(),
    })
}

const MISSION: MavMissionType = MavMissionType::MAV_MISSION_TYPE_MISSION;
const FENCE: MavMissionType = MavMissionType::MAV_MISSION_TYPE_FENCE;

#[tokio::test]
async fn download_retries_a_lost_item_and_acks_at_the_end() {
    let items = mission_items(4);
    let (_link, service, vehicle) = rig(download_vehicle(items.clone(), MISSION, Some(2))).await;
    let mut progress = service.subscribe_progress();

    let downloaded = service.download(MissionKind::Mission).await.unwrap();
    assert_eq!(downloaded.len(), 4);
    assert!(
        downloaded.iter().zip(&items).all(|(a, b)| close(a, b)),
        "{downloaded:?}"
    );
    assert_eq!(
        vehicle.count(|m| matches!(m, MavMessage::MISSION_REQUEST_INT(r) if r.seq == 2)),
        2,
        "the lost item is requested again"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        vehicle.count(|m| matches!(m, MavMessage::MISSION_ACK(a) if a.mavtype == MavMissionResult::MAV_MISSION_ACCEPTED)),
        1
    );

    let mut events = Vec::new();
    while let Ok(p) = progress.try_recv() {
        events.push(p);
    }
    assert!(events
        .iter()
        .all(|p| p.phase == MissionPhase::Download && p.kind == MissionKind::Mission));
    assert!(events
        .iter()
        .any(|p| p.total == 4 && p.done == 4 && !p.finished));
    let last = events.last().unwrap();
    assert!(last.finished && last.error.is_none() && last.done == 4);
}

#[tokio::test]
async fn download_repeats_the_list_request_when_the_count_is_lost() {
    let mut first = true;
    let inner = download_vehicle(mission_items(2), MISSION, None);
    let mut inner = inner;
    let handler: Handler = Box::new(move |m| {
        if matches!(m, MavMessage::MISSION_REQUEST_LIST(_)) && first {
            first = false;
            return Vec::new();
        }
        inner(m)
    });
    let (_link, service, vehicle) = rig(handler).await;
    assert_eq!(
        service.download(MissionKind::Mission).await.unwrap().len(),
        2
    );
    assert_eq!(
        vehicle.count(|m| matches!(m, MavMessage::MISSION_REQUEST_LIST(_))),
        2
    );
}

#[tokio::test]
async fn download_times_out_without_a_count_and_reports_it() {
    let (_link, service, _vehicle) = rig(silent()).await;
    let mut progress = service.subscribe_progress();
    let error = service.download(MissionKind::Mission).await.unwrap_err();
    assert!(matches!(error, MissionError::NoCount), "{error}");
    let mut last = None;
    while let Ok(p) = progress.try_recv() {
        last = Some(p);
    }
    let last = last.unwrap();
    assert!(last.finished);
    assert!(last.error.unwrap().contains("MISSION_COUNT"));
}

#[tokio::test]
async fn download_gives_up_on_an_item_that_never_arrives() {
    let handler: Handler = Box::new(|m| match m {
        MavMessage::MISSION_REQUEST_LIST(_) => {
            vec![MavMessage::MISSION_COUNT(MISSION_COUNT_DATA {
                count: 2,
                ..MISSION_COUNT_DATA::default()
            })]
        }
        _ => Vec::new(),
    });
    let (_link, service, _vehicle) = rig(handler).await;
    assert!(matches!(
        service.download(MissionKind::Mission).await,
        Err(MissionError::ItemTimeout { seq: 0 })
    ));
}

#[tokio::test]
async fn download_of_a_refused_list_is_an_error() {
    let handler: Handler = Box::new(|m| match m {
        MavMessage::MISSION_REQUEST_LIST(_) => {
            vec![ack(MavMissionResult::MAV_MISSION_DENIED, MISSION)]
        }
        _ => Vec::new(),
    });
    let (_link, service, _vehicle) = rig(handler).await;
    let error = service.download(MissionKind::Mission).await.unwrap_err();
    assert!(error.to_string().contains("denied"), "{error}");
}

#[tokio::test]
async fn upload_serves_the_requests_in_order_with_degrees_as_e7() {
    let store = Arc::new(Mutex::new(Vec::new()));
    let (_link, service, _vehicle) = rig(upload_vehicle(Style::Int, MISSION, store.clone())).await;
    let items = mission_items(5);
    service.upload(MissionKind::Mission, &items).await.unwrap();
    let received = store.lock().unwrap().clone();
    assert_eq!(received.len(), 5);
    for (sent, got) in items.iter().zip(&received) {
        assert_eq!(got.seq, sent.seq);
        assert_eq!(got.x, (sent.x * 1e7).round() as i32);
        assert_eq!(got.y, (sent.y * 1e7).round() as i32);
        assert_eq!(got.z, sent.z as f32);
        assert_eq!(got.mission_type, MISSION);
        assert_eq!(got.current, 0);
        assert_eq!(got.autocontinue, 1);
    }
    assert_eq!(received[0].frame, MavFrame::MAV_FRAME_GLOBAL);
    assert_eq!(received[1].frame, MavFrame::MAV_FRAME_GLOBAL_RELATIVE_ALT);
    assert_eq!(received[4].command, MavCmd::MAV_CMD_NAV_RETURN_TO_LAUNCH);
}

#[tokio::test]
async fn upload_numbers_items_by_position() {
    let store = Arc::new(Mutex::new(Vec::new()));
    let (_link, service, _vehicle) = rig(upload_vehicle(Style::Int, MISSION, store.clone())).await;
    let mut items = mission_items(3);
    items[0].seq = 40;
    items[2].seq = 7;
    service.upload(MissionKind::Mission, &items).await.unwrap();
    let seqs: Vec<u16> = store.lock().unwrap().iter().map(|i| i.seq).collect();
    assert_eq!(seqs, [0, 1, 2]);
}

#[tokio::test]
async fn upload_answers_the_legacy_mission_request_too() {
    let store = Arc::new(Mutex::new(Vec::new()));
    let (_link, service, _vehicle) =
        rig(upload_vehicle(Style::Legacy, MISSION, store.clone())).await;
    service
        .upload(MissionKind::Mission, &mission_items(3))
        .await
        .unwrap();
    assert_eq!(store.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn fence_uploads_carry_their_mission_type() {
    let store = Arc::new(Mutex::new(Vec::new()));
    let (_link, service, vehicle) = rig(upload_vehicle(Style::Int, FENCE, store.clone())).await;
    service
        .upload(MissionKind::Fence, &mission_items(3))
        .await
        .unwrap();
    assert!(store
        .lock()
        .unwrap()
        .iter()
        .all(|i| i.mission_type == FENCE));
    assert_eq!(
        vehicle.count(|m| matches!(m, MavMessage::MISSION_COUNT(c) if c.mission_type == FENCE)),
        1
    );
}

#[tokio::test]
async fn an_empty_upload_is_accepted_by_the_ack() {
    let store = Arc::new(Mutex::new(Vec::new()));
    let (_link, service, _vehicle) = rig(upload_vehicle(Style::Int, MISSION, store.clone())).await;
    service.upload(MissionKind::Mission, &[]).await.unwrap();
}

#[tokio::test]
async fn upload_no_space_is_fatal() {
    let handler: Handler = Box::new(|m| match m {
        MavMessage::MISSION_COUNT(_) => vec![ack(MavMissionResult::MAV_MISSION_NO_SPACE, MISSION)],
        _ => Vec::new(),
    });
    let (_link, service, _vehicle) = rig(handler).await;
    let error = service
        .upload(MissionKind::Mission, &mission_items(3))
        .await
        .unwrap_err();
    assert!(matches!(error, MissionError::Refused(_)), "{error}");
    assert!(error.to_string().contains("no space"), "{error}");
}

#[tokio::test]
async fn upload_tolerates_invalid_sequence_acks() {
    let store = Arc::new(Mutex::new(Vec::new()));
    let mut inner = upload_vehicle(Style::Int, MISSION, store.clone());
    let mut noisy = true;
    let handler: Handler = Box::new(move |m| {
        let mut replies = inner(m);
        if matches!(m, MavMessage::MISSION_ITEM_INT(i) if i.seq == 0) && noisy {
            noisy = false;
            replies.insert(
                0,
                ack(MavMissionResult::MAV_MISSION_INVALID_SEQUENCE, MISSION),
            );
        }
        replies
    });
    let (_link, service, _vehicle) = rig(handler).await;
    service
        .upload(MissionKind::Mission, &mission_items(3))
        .await
        .unwrap();
    assert_eq!(store.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn an_early_accepted_ack_does_not_end_the_upload() {
    let store = Arc::new(Mutex::new(Vec::new()));
    let mut inner = upload_vehicle(Style::Int, MISSION, store.clone());
    let mut stale = true;
    let handler: Handler = Box::new(move |m| {
        let mut replies = inner(m);
        if matches!(m, MavMessage::MISSION_COUNT(_)) && stale {
            stale = false;
            replies.insert(0, ack(MavMissionResult::MAV_MISSION_ACCEPTED, MISSION));
        }
        replies
    });
    let (_link, service, _vehicle) = rig(handler).await;
    service
        .upload(MissionKind::Mission, &mission_items(3))
        .await
        .unwrap();
    assert_eq!(store.lock().unwrap().len(), 3, "all items still delivered");
}

#[tokio::test]
async fn upload_resends_the_count_then_gives_up() {
    let (_link, service, vehicle) = rig(silent()).await;
    let error = service
        .upload(MissionKind::Mission, &mission_items(2))
        .await
        .unwrap_err();
    assert!(matches!(error, MissionError::NoRequest), "{error}");
    assert_eq!(
        vehicle.count(|m| matches!(m, MavMessage::MISSION_COUNT(_))),
        3,
        "one send plus two retries"
    );
}

#[tokio::test]
async fn upload_that_stalls_after_an_item_resends_it_then_fails() {
    let handler: Handler = Box::new(|m| match m {
        MavMessage::MISSION_COUNT(_) => {
            vec![MavMessage::MISSION_REQUEST_INT(MISSION_REQUEST_INT_DATA {
                seq: 0,
                mission_type: MISSION,
                ..MISSION_REQUEST_INT_DATA::default()
            })]
        }
        _ => Vec::new(),
    });
    let (_link, service, vehicle) = rig(handler).await;
    let error = service
        .upload(MissionKind::Mission, &mission_items(3))
        .await
        .unwrap_err();
    assert!(
        matches!(error, MissionError::UploadStalled { seq: 0 }),
        "{error}"
    );
    assert_eq!(
        vehicle.count(|m| matches!(m, MavMessage::MISSION_ITEM_INT(i) if i.seq == 0)),
        4,
        "the item and three resends"
    );
}

#[tokio::test]
async fn a_request_beyond_the_list_is_an_error() {
    let handler: Handler = Box::new(|m| match m {
        MavMessage::MISSION_COUNT(_) => {
            vec![MavMessage::MISSION_REQUEST_INT(MISSION_REQUEST_INT_DATA {
                seq: 9,
                mission_type: MISSION,
                ..MISSION_REQUEST_INT_DATA::default()
            })]
        }
        _ => Vec::new(),
    });
    let (_link, service, _vehicle) = rig(handler).await;
    assert!(matches!(
        service
            .upload(MissionKind::Mission, &mission_items(2))
            .await,
        Err(MissionError::BadRequest {
            requested: 9,
            count: 2
        })
    ));
}

#[tokio::test]
async fn unknown_commands_and_frames_are_rejected_before_anything_is_sent() {
    let (_link, service, vehicle) = rig(silent()).await;
    let mut items = mission_items(3);
    items[1].command = 60_000;
    assert!(matches!(
        service.upload(MissionKind::Mission, &items).await,
        Err(MissionError::UnknownCommand {
            seq: 1,
            command: 60_000
        })
    ));
    let mut items = mission_items(3);
    items[2].frame = 200;
    assert!(matches!(
        service.upload(MissionKind::Mission, &items).await,
        Err(MissionError::UnknownFrame { seq: 2, frame: 200 })
    ));
    assert_eq!(
        vehicle.count(|m| matches!(m, MavMessage::MISSION_COUNT(_))),
        0
    );
}

#[tokio::test]
async fn clear_waits_for_the_ack() {
    let handler: Handler = Box::new(|m| match m {
        MavMessage::MISSION_CLEAR_ALL(c) => {
            vec![ack(MavMissionResult::MAV_MISSION_ACCEPTED, c.mission_type)]
        }
        _ => Vec::new(),
    });
    let (_link, service, vehicle) = rig(handler).await;
    service.clear(MissionKind::Rally).await.unwrap();
    assert_eq!(
        vehicle.count(|m| matches!(m, MavMessage::MISSION_CLEAR_ALL(c) if c.mission_type == MavMissionType::MAV_MISSION_TYPE_RALLY)),
        1
    );
}

#[tokio::test]
async fn clear_without_an_answer_or_when_denied_is_an_error() {
    let (_link, service, _vehicle) = rig(silent()).await;
    assert!(matches!(
        service.clear(MissionKind::Mission).await,
        Err(MissionError::NoAck)
    ));
    let handler: Handler = Box::new(|m| match m {
        MavMessage::MISSION_CLEAR_ALL(_) => {
            vec![ack(MavMissionResult::MAV_MISSION_DENIED, MISSION)]
        }
        _ => Vec::new(),
    });
    let (_link, service, _vehicle) = rig(handler).await;
    assert!(matches!(
        service.clear(MissionKind::Mission).await,
        Err(MissionError::Refused(_))
    ));
}

#[tokio::test]
async fn only_one_transfer_at_a_time() {
    let (link, service, _vehicle) = rig(silent()).await;
    let slow = MissionService::new(link.clone()).with_timing(MissionTiming {
        count_timeout: Duration::from_millis(600),
        ..quick()
    });
    let running = {
        let slow = slow.clone();
        tokio::spawn(async move { slow.download(MissionKind::Mission).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(matches!(
        slow.upload(MissionKind::Mission, &[]).await,
        Err(MissionError::Busy)
    ));
    assert!(matches!(
        slow.clear(MissionKind::Mission).await,
        Err(MissionError::Busy)
    ));
    assert!(running.await.unwrap().is_err());
    // Idle again: the next operation is allowed to start (and fails on its own merits).
    assert!(matches!(
        service.clear(MissionKind::Mission).await,
        Err(MissionError::NoAck)
    ));
}

#[tokio::test]
async fn without_a_vehicle_nothing_is_sent() {
    let link = LinkManager::new();
    let service = MissionService::new(link.clone());
    for error in [
        service.download(MissionKind::Mission).await.unwrap_err(),
        service
            .upload(MissionKind::Mission, &mission_items(2))
            .await
            .unwrap_err(),
        service.clear(MissionKind::Mission).await.unwrap_err(),
    ] {
        assert_eq!(error.to_string(), "no vehicle connected; nothing was sent");
    }
}

#[tokio::test]
async fn mission_items_survive_a_replay_round_trip() {
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
    let service = MissionService::new(link.clone());

    let initial = service.download(MissionKind::Mission).await.unwrap();
    assert_eq!(initial.len(), 1, "a vehicle always reports home");
    assert!(initial[0].x != 0.0 && initial[0].x.abs() <= 90.0);
    assert!(service
        .download(MissionKind::Fence)
        .await
        .unwrap()
        .is_empty());

    let items = mission_items(6);
    service.upload(MissionKind::Mission, &items).await.unwrap();
    let back = service.download(MissionKind::Mission).await.unwrap();
    assert_eq!(back.len(), 6);
    assert!(
        back.iter().zip(&items).all(|(a, b)| close(a, b)),
        "{back:?}"
    );

    let fence = mission_items(4);
    service.upload(MissionKind::Fence, &fence).await.unwrap();
    assert_eq!(service.download(MissionKind::Fence).await.unwrap().len(), 4);
    assert_eq!(
        service.download(MissionKind::Mission).await.unwrap().len(),
        6,
        "kinds are separate"
    );

    service.clear(MissionKind::Fence).await.unwrap();
    assert!(service
        .download(MissionKind::Fence)
        .await
        .unwrap()
        .is_empty());
    service.clear(MissionKind::Mission).await.unwrap();
    assert_eq!(
        service.download(MissionKind::Mission).await.unwrap().len(),
        1,
        "home survives"
    );
}
