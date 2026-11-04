//! Drives the real `LinkManager` against a fake vehicle over UDP loopback.
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_link::{ConnectionConfig, LinkError, LinkEvent, LinkManager, Timing};
use control_station_mavlink::{
    dialect::{
        MavAutopilot, MavModeFlag, MavResult, MavSeverity, MavState, MavType, ATTITUDE_DATA,
        COMMAND_ACK_DATA, HEARTBEAT_DATA, STATUSTEXT_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion, Parser,
};
use tokio::{net::UdpSocket, time::timeout};

struct FakeVehicle {
    sequence: Arc<AtomicU8>,
    seen: Arc<Mutex<Vec<String>>>,
    sending: Arc<AtomicBool>,
}

impl FakeVehicle {
    /// `reply` is how the vehicle answers commands (`None` = stays silent).
    async fn start(gcs_port: u16, reply: Option<MavResult>) -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let vehicle = Self {
            sequence: Arc::new(AtomicU8::new(0)),
            seen: Arc::new(Mutex::new(Vec::new())),
            sending: Arc::new(AtomicBool::new(true)),
        };
        let frame = {
            let sequence = vehicle.sequence.clone();
            move |message: MavMessage| {
                encode(
                    MavHeader {
                        system_id: 1,
                        component_id: 1,
                        sequence: sequence.fetch_add(1, Ordering::Relaxed),
                    },
                    &message,
                    MavlinkVersion::V2,
                )
            }
        };
        let target = format!("127.0.0.1:{gcs_port}");

        let (tx_socket, tx_frame, tx_target, sending) = (
            socket.clone(),
            frame.clone(),
            target.clone(),
            vehicle.sending.clone(),
        );
        tokio::spawn(async move {
            loop {
                if sending.load(Ordering::Relaxed) {
                    for message in [heartbeat(true, 5), attitude(), statustext("Hello GCS")] {
                        tx_socket.send_to(&tx_frame(message), &tx_target).await.ok();
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        let (rx_socket, seen) = (socket, vehicle.seen.clone());
        tokio::spawn(async move {
            let mut parser = Parser::new();
            let mut buf = [0u8; 2048];
            loop {
                let Ok(n) = rx_socket.recv(&mut buf).await else {
                    return;
                };
                for packet in parser.push(&buf[..n]) {
                    use control_station_mavlink::Message;
                    seen.lock()
                        .unwrap()
                        .push(packet.message.message_name().to_owned());
                    let command = match &packet.message {
                        MavMessage::COMMAND_LONG(c) => Some(c.command),
                        MavMessage::COMMAND_INT(c) => Some(c.command),
                        _ => None,
                    };
                    if let (Some(command), Some(result)) = (command, reply) {
                        let ack = MavMessage::COMMAND_ACK(COMMAND_ACK_DATA {
                            command,
                            result,
                            ..COMMAND_ACK_DATA::default()
                        });
                        rx_socket.send_to(&frame(ack), &target).await.ok();
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

fn heartbeat(armed: bool, mode: u32) -> MavMessage {
    MavMessage::HEARTBEAT(HEARTBEAT_DATA {
        custom_mode: mode,
        mavtype: MavType::MAV_TYPE_QUADROTOR,
        autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
        base_mode: if armed {
            MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
        } else {
            MavModeFlag::empty()
        },
        system_status: MavState::MAV_STATE_ACTIVE,
        mavlink_version: 3,
    })
}

fn attitude() -> MavMessage {
    MavMessage::ATTITUDE(ATTITUDE_DATA {
        roll: std::f32::consts::FRAC_PI_6,
        ..ATTITUDE_DATA::default()
    })
}

fn statustext(text: &str) -> MavMessage {
    MavMessage::STATUSTEXT(STATUSTEXT_DATA {
        severity: MavSeverity::MAV_SEVERITY_WARNING,
        text: text.into(),
        ..STATUSTEXT_DATA::default()
    })
}

async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let start = tokio::time::Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn connected_manager(reply: Option<MavResult>, timing: Timing) -> (LinkManager, FakeVehicle) {
    let manager = LinkManager::with_timing(timing);
    manager
        .connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let vehicle = FakeVehicle::start(manager.local_port().unwrap(), reply).await;
    wait_until("vehicle heartbeat", || manager.telemetry().connected).await;
    (manager, vehicle)
}

fn quick_timing() -> Timing {
    Timing {
        ack_timeout: Duration::from_millis(150),
        ack_attempts: 2,
        ..Timing::default()
    }
}

#[tokio::test]
async fn telemetry_events_and_gcs_traffic() {
    let manager = LinkManager::new();
    let mut events = manager.subscribe();
    manager
        .connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let vehicle = FakeVehicle::start(manager.local_port().unwrap(), None).await;

    let (mut got_telemetry, mut got_text) = (None, None);
    timeout(Duration::from_secs(5), async {
        while got_telemetry.is_none() || got_text.is_none() {
            match events.recv().await.unwrap() {
                LinkEvent::Telemetry(t) if t.connected && t.fields.contains_key("roll") => {
                    got_telemetry = Some(t);
                }
                LinkEvent::StatusText(t) => got_text = Some(t),
                _ => {}
            }
        }
    })
    .await
    .expect("telemetry and status text events");

    let t = got_telemetry.unwrap();
    assert!(t.armed);
    assert_eq!(t.mode.as_deref(), Some("Loiter"));
    assert_eq!(t.vehicle_type, "Copter");
    assert!(t.available_modes.iter().any(|m| m == "RTL"));
    assert!((t.fields["roll"] - 30.0).abs() < 0.01);
    assert_eq!(t.vehicle.unwrap().system_id, 1);
    let text = got_text.unwrap();
    assert_eq!((text.text.as_str(), text.severity), ("Hello GCS", 4));

    wait_until("GCS heartbeat", || vehicle.saw("HEARTBEAT")).await;
    wait_until("data stream requests", || {
        vehicle.saw("REQUEST_DATA_STREAM")
    })
    .await;
    assert!(manager.link_info().stats.packets_rx > 0);
}

#[tokio::test]
async fn arm_succeeds_when_accepted_and_reports_denial() {
    let (manager, vehicle) =
        connected_manager(Some(MavResult::MAV_RESULT_ACCEPTED), quick_timing()).await;
    assert_eq!(manager.arm(false).await.unwrap(), "Accepted");
    assert_eq!(manager.set_mode("RTL").await.unwrap(), "Accepted");
    assert!(matches!(
        manager.set_mode("Nonsense").await,
        Err(LinkError::UnknownMode(_))
    ));
    assert!(vehicle.saw("COMMAND_LONG"));
    drop(vehicle);

    let (manager, _vehicle) =
        connected_manager(Some(MavResult::MAV_RESULT_DENIED), quick_timing()).await;
    match manager.arm(false).await {
        Err(LinkError::Denied(reason)) => assert_eq!(reason, "denied"),
        other => panic!("expected denial, got {other:?}"),
    }
}

#[tokio::test]
async fn silent_vehicle_means_no_ack() {
    let (manager, _vehicle) = connected_manager(None, quick_timing()).await;
    assert!(matches!(manager.reboot().await, Err(LinkError::NoAck)));
}

#[tokio::test]
async fn commands_are_refused_without_a_vehicle() {
    let manager = LinkManager::new();
    assert!(matches!(
        manager.arm(false).await,
        Err(LinkError::NoVehicle)
    ));
    manager
        .connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    assert!(matches!(manager.reboot().await, Err(LinkError::NoVehicle)));
    manager.disconnect();
    assert!(matches!(
        manager.do_action("Mission_Start").await,
        Err(LinkError::NoVehicle)
    ));
}

#[tokio::test]
async fn heartbeat_timeout_marks_vehicle_disconnected() {
    let timing = Timing {
        heartbeat_timeout: Duration::from_millis(500),
        ..Timing::default()
    };
    let (manager, vehicle) = connected_manager(None, timing).await;
    vehicle.sending.store(false, Ordering::Relaxed);
    wait_until("heartbeat timeout", || !manager.telemetry().connected).await;
    assert!(!manager.telemetry().armed);
    vehicle.sending.store(true, Ordering::Relaxed);
    wait_until("recovery", || manager.telemetry().connected).await;
}

#[tokio::test]
async fn sequence_gaps_count_as_lost_packets() {
    let manager = LinkManager::new();
    manager
        .connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let port = manager.local_port().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    for sequence in [0u8, 1, 2, 5, 6] {
        let frame = encode(
            MavHeader {
                system_id: 1,
                component_id: 1,
                sequence,
            },
            &heartbeat(false, 0),
            MavlinkVersion::V2,
        );
        socket.send_to(&frame, ("127.0.0.1", port)).await.unwrap();
    }
    wait_until("five packets", || manager.link_info().stats.packets_rx == 5).await;
    assert_eq!(manager.link_info().stats.packets_lost, 2);
}

#[tokio::test]
async fn failed_connection_reports_error_state() {
    let manager = LinkManager::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let result = manager
        .connect(ConnectionConfig::Tcp {
            host: "127.0.0.1".into(),
            port,
        })
        .await;
    assert!(result.is_err());
    let info = manager.link_info();
    assert!(info.error.is_some());
    assert_eq!(format!("{:?}", info.state), "Disconnected");
}

#[tokio::test]
async fn replay_of_missing_file_is_an_error() {
    let manager = LinkManager::new();
    let result = manager
        .connect(ConnectionConfig::Replay {
            path: "does/not/exist.BIN".into(),
            speed: None,
        })
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn replay_of_the_sample_log_produces_a_vehicle() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../exemplos/Teste 02 - tunning.BIN"
    );
    if !std::path::Path::new(path).exists() {
        eprintln!("skipping: sample log not present");
        return;
    }
    let manager = LinkManager::new();
    manager
        .connect(ConnectionConfig::Replay {
            path: path.into(),
            speed: Some(50.0),
        })
        .await
        .unwrap();
    wait_until("replayed vehicle", || {
        let t = manager.telemetry();
        t.connected && t.position.is_some() && t.fields.contains_key("roll")
    })
    .await;
    let t = manager.telemetry();
    assert_eq!(t.vehicle_type, "Copter");
    let position = t.position.unwrap();
    assert!((-90.0..=90.0).contains(&position.lat));
    // Commands cannot reach a log: the replay answers "unsupported" instead of timing out.
    assert!(matches!(
        manager.reboot().await,
        Err(LinkError::Denied("unsupported"))
    ));
    manager.disconnect();
    assert!(!manager.telemetry().connected);
}
