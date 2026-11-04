//! Parameter download and write against a fake vehicle over UDP loopback, and against the Replay.
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_link::{
    ConnectionConfig, LinkManager, ParamError, ParamProgress, ParamService, ParamTiming,
};
use control_station_mavlink::{
    dialect::{
        MavAutopilot, MavModeFlag, MavParamType, MavState, MavType, HEARTBEAT_DATA,
        PARAM_VALUE_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion, Parser,
};
use tokio::{net::UdpSocket, sync::broadcast};

#[derive(Clone, Default)]
struct Behavior {
    /// Index left out the first time the list is streamed.
    drop_index: Option<u16>,
    /// The first PARAM_REQUEST_LIST is ignored completely.
    ignore_first_list: bool,
    /// PARAM_REQUEST_READ for this index is never answered.
    never_answer_read: Option<u16>,
    /// PARAM_SET gets no echo.
    silent_set: bool,
}

struct FakeVehicle {
    seen: Arc<Mutex<Vec<String>>>,
    reads: Arc<AtomicUsize>,
    lists: Arc<AtomicUsize>,
}

fn table() -> Vec<(String, f32, MavParamType)> {
    let mut items: Vec<(String, f32, MavParamType)> = vec![
        (
            "ARMING_CHECK".into(),
            1.0,
            MavParamType::MAV_PARAM_TYPE_INT32,
        ),
        (
            "ATC_RAT_RLL_P".into(),
            0.135,
            MavParamType::MAV_PARAM_TYPE_REAL32,
        ),
        (
            "RC10_MIN".into(),
            1000.0,
            MavParamType::MAV_PARAM_TYPE_INT16,
        ),
        ("RC2_MIN".into(), 1100.0, MavParamType::MAV_PARAM_TYPE_INT16),
    ];
    for i in 0..26 {
        items.push((
            format!("TEST_{i:02}"),
            i as f32,
            MavParamType::MAV_PARAM_TYPE_INT32,
        ));
    }
    items
}

fn header(sequence: u8) -> MavHeader {
    MavHeader {
        system_id: 1,
        component_id: 1,
        sequence,
    }
}

fn value_message(items: &[(String, f32, MavParamType)], index: usize) -> MavMessage {
    let (name, value, kind) = &items[index];
    MavMessage::PARAM_VALUE(PARAM_VALUE_DATA {
        param_value: *value,
        param_count: items.len() as u16,
        param_index: index as u16,
        param_id: name.as_str().into(),
        param_type: *kind,
    })
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

impl FakeVehicle {
    async fn start(gcs_port: u16, behavior: Behavior) -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let target = format!("127.0.0.1:{gcs_port}");
        let sequence = Arc::new(AtomicU8::new(0));
        let frame = {
            let sequence = sequence.clone();
            move |message: MavMessage| {
                encode(
                    header(sequence.fetch_add(1, Ordering::Relaxed)),
                    &message,
                    MavlinkVersion::V2,
                )
            }
        };
        let vehicle = Self {
            seen: Arc::new(Mutex::new(Vec::new())),
            reads: Arc::new(AtomicUsize::new(0)),
            lists: Arc::new(AtomicUsize::new(0)),
        };

        {
            let (socket, frame, target) = (socket.clone(), frame.clone(), target.clone());
            tokio::spawn(async move {
                let heartbeat = MavMessage::HEARTBEAT(HEARTBEAT_DATA {
                    custom_mode: 5,
                    mavtype: MavType::MAV_TYPE_QUADROTOR,
                    autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
                    base_mode: MavModeFlag::empty(),
                    system_status: MavState::MAV_STATE_ACTIVE,
                    mavlink_version: 3,
                });
                loop {
                    socket
                        .send_to(&frame(heartbeat.clone()), &target)
                        .await
                        .ok();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
        }

        let (seen, reads, lists) = (
            vehicle.seen.clone(),
            vehicle.reads.clone(),
            vehicle.lists.clone(),
        );
        tokio::spawn(async move {
            use control_station_mavlink::Message;
            let mut items = table();
            let mut dropped_once = false;
            let mut parser = Parser::new();
            let mut buf = [0u8; 2048];
            loop {
                let Ok(n) = socket.recv(&mut buf).await else {
                    return;
                };
                for packet in parser.push(&buf[..n]) {
                    seen.lock()
                        .unwrap()
                        .push(packet.message.message_name().to_owned());
                    match packet.message {
                        MavMessage::PARAM_REQUEST_LIST(_) => {
                            let first = lists.fetch_add(1, Ordering::SeqCst) == 0;
                            if first && behavior.ignore_first_list {
                                continue;
                            }
                            for index in 0..items.len() {
                                if Some(index as u16) == behavior.drop_index && !dropped_once {
                                    continue;
                                }
                                let message = value_message(&items, index);
                                socket.send_to(&frame(message), &target).await.ok();
                            }
                            dropped_once = true;
                        }
                        MavMessage::PARAM_REQUEST_READ(r) => {
                            reads.fetch_add(1, Ordering::SeqCst);
                            let index = r.param_index.max(0) as usize;
                            if Some(index as u16) == behavior.never_answer_read
                                || index >= items.len()
                            {
                                continue;
                            }
                            socket
                                .send_to(&frame(value_message(&items, index)), &target)
                                .await
                                .ok();
                        }
                        MavMessage::PARAM_SET(set) => {
                            if behavior.silent_set {
                                continue;
                            }
                            let name = text(&*set.param_id);
                            if let Some(index) = items.iter().position(|(n, _, _)| *n == name) {
                                let integer =
                                    !matches!(items[index].2, MavParamType::MAV_PARAM_TYPE_REAL32);
                                items[index].1 = if integer {
                                    set.param_value.round()
                                } else {
                                    set.param_value
                                };
                                socket
                                    .send_to(&frame(value_message(&items, index)), &target)
                                    .await
                                    .ok();
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
        vehicle
    }

    fn saw(&self, name: &str) -> bool {
        self.seen.lock().unwrap().iter().any(|n| n == name)
    }
}

fn fast() -> ParamTiming {
    ParamTiming {
        idle_timeout: Duration::from_millis(400),
        read_round: Duration::from_millis(100),
        read_batch: 10,
        max_idle_rounds: 3,
        echo_timeout: Duration::from_millis(150),
        set_attempts: 2,
    }
}

async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let start = tokio::time::Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn setup(behavior: Behavior) -> (LinkManager, ParamService, FakeVehicle) {
    let manager = LinkManager::new();
    manager
        .connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let vehicle = FakeVehicle::start(manager.local_port().unwrap(), behavior).await;
    wait_until("vehicle heartbeat", || manager.telemetry().connected).await;
    let service = ParamService::new(manager.clone()).with_timing(fast());
    (manager, service, vehicle)
}

fn drain(rx: &mut broadcast::Receiver<ParamProgress>) -> Vec<ParamProgress> {
    let mut out = Vec::new();
    while let Ok(p) = rx.try_recv() {
        out.push(p);
    }
    out
}

#[tokio::test]
async fn downloads_everything_including_a_dropped_value() {
    let (_manager, service, vehicle) = setup(Behavior {
        drop_index: Some(7),
        ..Behavior::default()
    })
    .await;
    let mut progress = service.subscribe_progress();
    assert_eq!(service.refresh().await.unwrap(), 30);

    let list = service.list();
    assert_eq!(list.len(), 30);
    let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"TEST_07"),
        "the dropped parameter was re-requested"
    );
    assert!(vehicle.reads.load(Ordering::SeqCst) >= 1);
    // Natural order: RC2_MIN before RC10_MIN, ARMING_CHECK first.
    assert_eq!(names[0], "ARMING_CHECK");
    let rc2 = names.iter().position(|n| *n == "RC2_MIN").unwrap();
    let rc10 = names.iter().position(|n| *n == "RC10_MIN").unwrap();
    assert!(rc2 < rc10);

    let events = drain(&mut progress);
    let last = events.last().expect("progress events");
    assert!(last.done && last.error.is_none());
    assert_eq!((last.received, last.total), (30, 30));
}

#[tokio::test]
async fn an_ignored_list_request_is_repeated() {
    let (_manager, service, vehicle) = setup(Behavior {
        ignore_first_list: true,
        ..Behavior::default()
    })
    .await;
    assert_eq!(service.refresh().await.unwrap(), 30);
    assert!(vehicle.lists.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn gives_up_with_a_clear_error_when_a_value_never_arrives() {
    let (_manager, service, _vehicle) = setup(Behavior {
        drop_index: Some(3),
        never_answer_read: Some(3),
        ..Behavior::default()
    })
    .await;
    let mut progress = service.subscribe_progress();
    match service.refresh().await {
        Err(ParamError::Incomplete { received, total }) => assert_eq!((received, total), (29, 30)),
        other => panic!("expected Incomplete, got {other:?}"),
    }
    let last = drain(&mut progress).pop().unwrap();
    assert!(last.done);
    assert!(last.error.unwrap().contains("29 of 30"));
}

#[tokio::test]
async fn set_returns_the_echoed_value_and_updates_the_table() {
    let (_manager, service, vehicle) = setup(Behavior::default()).await;
    service.refresh().await.unwrap();

    let echoed = service.set("ATC_RAT_RLL_P", 0.2).await.unwrap();
    assert!((echoed - 0.2).abs() < 1e-6);
    let stored = service
        .list()
        .into_iter()
        .find(|p| p.name == "ATC_RAT_RLL_P")
        .unwrap();
    assert!((stored.value - 0.2).abs() < 1e-6);
    assert!(vehicle.saw("PARAM_SET"));

    // The vehicle rounds integer parameters; the service reports what the vehicle really has.
    assert_eq!(service.set("ARMING_CHECK", 2.6).await.unwrap(), 3.0);
    let stored = service
        .list()
        .into_iter()
        .find(|p| p.name == "ARMING_CHECK")
        .unwrap();
    assert_eq!(stored.value, 3.0);
}

#[tokio::test]
async fn set_without_an_echo_fails() {
    let (_manager, service, _vehicle) = setup(Behavior {
        silent_set: true,
        ..Behavior::default()
    })
    .await;
    service.refresh().await.unwrap();
    assert!(matches!(
        service.set("ARMING_CHECK", 0.0).await,
        Err(ParamError::NoEcho(name)) if name == "ARMING_CHECK"
    ));
    let stored = service
        .list()
        .into_iter()
        .find(|p| p.name == "ARMING_CHECK")
        .unwrap();
    assert_eq!(
        stored.value, 1.0,
        "an unconfirmed write must not change the table"
    );
}

#[tokio::test]
async fn set_rejects_unknown_and_overlong_names() {
    let (_manager, service, _vehicle) = setup(Behavior::default()).await;
    assert!(
        matches!(
            service.set("ARMING_CHECK", 0.0).await,
            Err(ParamError::Unknown(_))
        ),
        "before any download nothing is known"
    );
    service.refresh().await.unwrap();
    assert!(matches!(
        service.set("NOT_A_PARAM", 1.0).await,
        Err(ParamError::Unknown(_))
    ));
    assert!(matches!(
        service.set("THIS_NAME_IS_MUCH_TOO_LONG", 1.0).await,
        Err(ParamError::NameTooLong(_))
    ));
}

#[tokio::test]
async fn disconnecting_clears_the_table() {
    let (manager, service, _vehicle) = setup(Behavior::default()).await;
    service.refresh().await.unwrap();
    assert_eq!(service.list().len(), 30);
    manager.disconnect();
    wait_until("table cleared", || service.list().is_empty()).await;
    assert!(matches!(service.refresh().await, Err(ParamError::Link(_))));
}

#[tokio::test]
async fn refresh_without_a_vehicle_is_an_error() {
    let service = ParamService::new(LinkManager::new());
    assert!(matches!(service.refresh().await, Err(ParamError::Link(_))));
    assert!(service.list().is_empty());
    assert!(service.metadata().await.is_empty());
}

const PDEF_SAMPLE: &str = r#"<paramfile><vehicles><parameters name="ArduCopter">
<param humanName="Arming check" name="ArduCopter:ARMING_CHECK" documentation="Checks prior to arming." user="Standard">
<field name="RebootRequired">True</field>
<values><value code="0">Disabled</value><value code="1">Enabled</value></values>
</param></parameters></vehicles></paramfile>"#;

#[tokio::test]
async fn metadata_is_fetched_once_and_cached() {
    let (_manager, service, _vehicle) = setup(Behavior::default()).await;
    let dir = std::env::temp_dir().join(format!("cs-params-meta-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fetches = Arc::new(AtomicUsize::new(0));
    let counter = fetches.clone();
    let service = service
        .with_cache_dir(dir.clone())
        .with_fetcher(move |url| {
            assert!(url.contains("ArduCopter"));
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(PDEF_SAMPLE.as_bytes().to_vec())
        });
    let meta = service.metadata().await;
    assert_eq!(meta["ARMING_CHECK"].display_name, "Arming check");
    assert!(meta["ARMING_CHECK"].reboot_required);
    assert_eq!(service.metadata().await.len(), 1);
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn metadata_failure_gives_an_empty_map() {
    let (_manager, service, _vehicle) = setup(Behavior::default()).await;
    let dir = std::env::temp_dir().join(format!("cs-params-nometa-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let service = service
        .with_cache_dir(dir)
        .with_fetcher(|_| Err("offline".to_owned()));
    assert!(service.metadata().await.is_empty());
}

#[tokio::test]
async fn param_files_round_trip_through_the_service() {
    let (_manager, service, _vehicle) = setup(Behavior::default()).await;
    service.refresh().await.unwrap();
    let text = service.format_file();
    assert!(
        text.starts_with("ARMING_CHECK,1\nATC_RAT_RLL_P,0.135\n"),
        "{text}"
    );
    let parsed = service.parse_file(&text);
    assert_eq!(parsed.params.len(), 30);
    assert!(parsed.skipped.is_empty());
}

fn sample_log() -> Option<PathBuf> {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../exemplos/Teste 02 - tunning.BIN"
    ));
    path.exists().then_some(path)
}

#[tokio::test]
async fn replay_serves_the_logged_parameters() {
    let Some(path) = sample_log() else {
        eprintln!("skipping: sample log not present");
        return;
    };
    let mut unique = std::collections::HashSet::new();
    let log = control_station_dataflash::Log::open(&path).unwrap();
    for record in log.records_named("PARM") {
        unique.insert(record.text("Name").unwrap().to_owned());
    }
    drop(log);

    let manager = LinkManager::new();
    manager
        .connect(ConnectionConfig::Replay {
            path: path.to_string_lossy().into_owned(),
            speed: Some(50.0),
        })
        .await
        .unwrap();
    wait_until("replayed vehicle", || manager.telemetry().connected).await;
    let service = ParamService::new(manager.clone());

    let count = service.refresh().await.unwrap();
    assert_eq!(count, unique.len());
    // The log has 1360 PARM records, but some parameters are logged again after changing.
    assert!(unique.len() > 1000, "{} distinct parameters", unique.len());
    let list = service.list();
    assert!(list.iter().any(|p| p.name == "ATC_RAT_RLL_P"));

    let target = list
        .iter()
        .find(|p| p.param_type.is_integer() && p.value < 1000.0)
        .expect("an integer parameter");
    let wanted = target.value + 1.0;
    assert_eq!(service.set(&target.name, wanted).await.unwrap(), wanted);
    let after = service
        .list()
        .into_iter()
        .find(|p| p.name == target.name)
        .unwrap();
    assert_eq!(after.value, wanted);
    manager.disconnect();
}
