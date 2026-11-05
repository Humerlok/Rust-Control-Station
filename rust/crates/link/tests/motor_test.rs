//! Motor test against a fake vehicle over UDP loopback.
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_commands::MotorThrottleType;
use control_station_link::{
    ConnectionConfig, LinkManager, MotorTestRequest, MotorTestService, ParamWriter, SpinParam,
};
use control_station_mavlink::{
    dialect::{
        MavAutopilot, MavCmd, MavModeFlag, MavResult, MavState, MavType, COMMAND_ACK_DATA,
        COMMAND_LONG_DATA, ESC_TELEMETRY_1_TO_4_DATA, HEARTBEAT_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion, Parser,
};
use tokio::net::UdpSocket;

struct Vehicle {
    socket: Arc<UdpSocket>,
    target: String,
    sequence: Arc<AtomicU8>,
    received: Arc<Mutex<Vec<MavMessage>>>,
    /// Answer DO_MOTOR_TEST with DENIED instead of ACCEPTED.
    deny: Arc<AtomicBool>,
}

fn frame(sequence: &AtomicU8, message: &MavMessage) -> Vec<u8> {
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

impl Vehicle {
    async fn start(gcs_port: u16, armed: bool) -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let vehicle = Self {
            socket: socket.clone(),
            target: format!("127.0.0.1:{gcs_port}"),
            sequence: Arc::new(AtomicU8::new(0)),
            received: Arc::new(Mutex::new(Vec::new())),
            deny: Arc::new(AtomicBool::new(false)),
        };
        let (tx, seq, target) = (
            socket.clone(),
            vehicle.sequence.clone(),
            vehicle.target.clone(),
        );
        tokio::spawn(async move {
            loop {
                let message = MavMessage::HEARTBEAT(HEARTBEAT_DATA {
                    custom_mode: 0,
                    mavtype: MavType::MAV_TYPE_QUADROTOR,
                    autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
                    base_mode: if armed {
                        MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
                    } else {
                        MavModeFlag::empty()
                    },
                    system_status: MavState::MAV_STATE_STANDBY,
                    mavlink_version: 3,
                });
                tx.send_to(&frame(&seq, &message), &target).await.ok();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        let (rx, log, seq, target, deny) = (
            socket,
            vehicle.received.clone(),
            vehicle.sequence.clone(),
            vehicle.target.clone(),
            vehicle.deny.clone(),
        );
        tokio::spawn(async move {
            let mut parser = Parser::new();
            let mut buf = [0u8; 2048];
            while let Ok(n) = rx.recv(&mut buf).await {
                for packet in parser.push(&buf[..n]) {
                    if let MavMessage::COMMAND_LONG(c) = &packet.message {
                        if c.command == MavCmd::MAV_CMD_DO_MOTOR_TEST {
                            let result = if deny.load(Ordering::Relaxed) {
                                MavResult::MAV_RESULT_DENIED
                            } else {
                                MavResult::MAV_RESULT_ACCEPTED
                            };
                            let ack = MavMessage::COMMAND_ACK(COMMAND_ACK_DATA {
                                command: c.command,
                                result,
                                ..COMMAND_ACK_DATA::default()
                            });
                            rx.send_to(&frame(&seq, &ack), &target).await.ok();
                        }
                    }
                    log.lock().unwrap().push(packet.message);
                }
            }
        });
        vehicle
    }

    async fn send(&self, message: MavMessage) {
        self.socket
            .send_to(&frame(&self.sequence, &message), &self.target)
            .await
            .unwrap();
    }

    fn command_longs(&self, command: MavCmd) -> Vec<COMMAND_LONG_DATA> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MavMessage::COMMAND_LONG(c) if c.command == command => Some(c.clone()),
                _ => None,
            })
            .collect()
    }
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

struct Rig {
    service: MotorTestService,
    vehicle: Vehicle,
    writes: Arc<Mutex<Vec<(String, f64)>>>,
}

async fn rig(armed: bool) -> Rig {
    let link = LinkManager::new();
    link.connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let vehicle = Vehicle::start(link.local_port().unwrap(), armed).await;
    wait_until("vehicle", || link.telemetry().connected).await;
    let writes = Arc::new(Mutex::new(Vec::new()));
    let log = writes.clone();
    let writer: ParamWriter = Arc::new(move |name, value| {
        let log = log.clone();
        Box::pin(async move {
            log.lock().unwrap().push((name, value));
            Ok(value)
        })
    });
    let service = MotorTestService::new(link, writer);
    let runner = service.clone();
    tokio::spawn(async move { runner.run().await });
    Rig {
        service,
        vehicle,
        writes,
    }
}

fn request(motor: u8, throttle: f32, seconds: f32) -> MotorTestRequest {
    MotorTestRequest {
        motor,
        throttle_type: MotorThrottleType::Percent,
        throttle,
        seconds,
        allow_high: false,
    }
}

#[tokio::test]
async fn one_motor_is_started_and_stopped() {
    let rig = rig(false).await;
    let message = rig.service.start(request(2, 5.0, 2.0)).await.unwrap();
    assert!(message.contains("Motor B"), "{message}");
    let sent = rig.vehicle.command_longs(MavCmd::MAV_CMD_DO_MOTOR_TEST);
    assert_eq!(sent.len(), 1);
    let c = &sent[0];
    assert_eq!((c.param1, c.param2, c.param3), (2.0, 0.0, 5.0));
    assert_eq!((c.param4, c.param5), (2.0, 0.0));
    assert!(rig.service.status().running);

    // Only one test at a time.
    let error = rig.service.start(request(1, 5.0, 2.0)).await.unwrap_err();
    assert!(error.to_string().contains("already running"), "{error}");

    rig.service.stop_all(4).await.unwrap();
    let sent = rig.vehicle.command_longs(MavCmd::MAV_CMD_DO_MOTOR_TEST);
    assert_eq!(sent.len(), 5, "1 start + 4 stops");
    assert!(sent[1..].iter().all(|c| c.param3 == 0.0 && c.param4 == 0.0));
    assert_eq!(
        sent[1..].iter().map(|c| c.param1 as u8).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert!(!rig.service.status().running);
    rig.service.start(request(1, 5.0, 1.0)).await.unwrap();
}

#[tokio::test]
async fn armed_vehicle_cannot_be_tested_but_can_be_stopped() {
    let rig = rig(true).await;
    let error = rig.service.start(request(1, 5.0, 2.0)).await.unwrap_err();
    assert!(error.to_string().contains("armed"), "{error}");
    assert!(rig
        .vehicle
        .command_longs(MavCmd::MAV_CMD_DO_MOTOR_TEST)
        .is_empty());
    assert!(rig.service.set_spin(SpinParam::Arm, 10.0).await.is_err());
    rig.service.stop_all(4).await.unwrap();
    assert_eq!(
        rig.vehicle
            .command_longs(MavCmd::MAV_CMD_DO_MOTOR_TEST)
            .len(),
        4
    );
}

#[tokio::test]
async fn refusals_send_nothing() {
    let rig = rig(false).await;
    assert!(rig.service.start(request(1, 40.0, 2.0)).await.is_err());
    assert!(rig.service.start(request(0, 5.0, 2.0)).await.is_err());
    assert!(rig.service.start(request(1, 5.0, 0.0)).await.is_err());
    assert!(rig
        .service
        .start_all(request(1, 5.0, 2.0), 0)
        .await
        .is_err());
    let mut confirmed = request(1, 40.0, 2.0);
    confirmed.allow_high = true;
    rig.service.start(confirmed).await.unwrap();
    assert_eq!(
        rig.vehicle
            .command_longs(MavCmd::MAV_CMD_DO_MOTOR_TEST)
            .len(),
        1,
        "only the confirmed high throttle went out"
    );
}

#[tokio::test]
async fn a_denied_test_is_an_error_and_is_not_running() {
    let rig = rig(false).await;
    rig.vehicle.deny.store(true, Ordering::Relaxed);
    let error = rig.service.start(request(1, 5.0, 2.0)).await.unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("denied"),
        "{error}"
    );
    assert!(!rig.service.status().running);
}

#[tokio::test]
async fn all_motors_use_one_command_each_and_the_sequence_uses_one() {
    let rig = rig(false).await;
    rig.service
        .start_all(request(0, 5.0, 1.0), 6)
        .await
        .unwrap();
    let sent = rig.vehicle.command_longs(MavCmd::MAV_CMD_DO_MOTOR_TEST);
    assert_eq!(sent.len(), 6);
    assert!(sent.iter().all(|c| c.param5 == 0.0 && c.param4 == 1.0));
    rig.service.stop_all(6).await.unwrap();

    rig.service
        .start_sequence(request(0, 5.0, 1.5), 6)
        .await
        .unwrap();
    let sent = rig.vehicle.command_longs(MavCmd::MAV_CMD_DO_MOTOR_TEST);
    let sequence = sent.last().unwrap();
    assert_eq!(
        (sequence.param1, sequence.param4, sequence.param5),
        (1.0, 1.5, 6.0)
    );
    assert!(rig.service.status().running);
}

#[tokio::test]
async fn esc_telemetry_fills_the_readings() {
    let rig = rig(false).await;
    rig.vehicle
        .send(MavMessage::ESC_TELEMETRY_1_TO_4(
            ESC_TELEMETRY_1_TO_4_DATA {
                voltage: [1200, 0, 1190, 0],
                current: [350, 0, 340, 0],
                rpm: [1000, 0, 2000, 0],
                temperature: [31, 0, 33, 0],
                ..ESC_TELEMETRY_1_TO_4_DATA::default()
            },
        ))
        .await;
    wait_until("ESC readings", || rig.service.status().escs.len() == 2).await;
    let escs = rig.service.status().escs;
    assert_eq!(escs[0].index, 1);
    assert_eq!(escs[0].rpm, Some(1000));
    assert_eq!(escs[0].voltage, Some(12.0));
    assert_eq!(escs[0].temperature, Some(31));
    assert_eq!(escs[1].index, 3);
    assert_eq!(escs[1].current, Some(3.4));
}

#[tokio::test]
async fn watching_the_escs_requests_their_messages() {
    let rig = rig(false).await;
    rig.service.watch_esc(true).unwrap();
    wait_until("interval requests", || {
        rig.vehicle
            .command_longs(MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL)
            .len()
            == 4
    })
    .await;
    let ids: Vec<u32> = rig
        .vehicle
        .command_longs(MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL)
        .iter()
        .map(|c| c.param1 as u32)
        .collect();
    assert_eq!(ids, vec![291, 11030, 11031, 11032]);
    rig.service.watch_esc(false).unwrap();
    wait_until("reset requests", || {
        rig.vehicle
            .command_longs(MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL)
            .len()
            == 8
    })
    .await;
    let last = rig
        .vehicle
        .command_longs(MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL)
        .pop()
        .unwrap();
    assert_eq!(last.param2, 0.0);
}

#[tokio::test]
async fn frames_come_from_the_layout_table_or_are_generic() {
    let rig = rig(false).await;
    let quad = rig.service.frame(Some(1), Some(1), None).unwrap();
    assert!(quad.known);
    assert_eq!(quad.class_name.as_deref(), Some("QUAD"));
    assert_eq!(quad.motors.len(), 4);
    assert_eq!(quad.motors[0].label, "A");
    assert_eq!(quad.motors[0].output, Some(1));
    assert_eq!(quad.motors[1].output, Some(4));
    assert_eq!(quad.motors[1].rotation.as_deref(), Some("CW"));

    let hexa = rig.service.frame(Some(2), Some(1), None).unwrap();
    assert_eq!(hexa.motors.len(), 6);
    // A known frame ignores the generic count.
    assert_eq!(
        rig.service
            .frame(Some(2), Some(1), Some(9))
            .unwrap()
            .motors
            .len(),
        6
    );

    let generic = rig.service.frame(None, None, None).unwrap();
    assert!(!generic.known);
    assert_eq!(generic.motors.len(), 4);
    assert_eq!(generic.motors[0].output, None);

    // A frame that is not in the table (class 17, scripting matrix) takes the user's count.
    let scripted = rig.service.frame(Some(17), Some(1), Some(8)).unwrap();
    assert!(!scripted.known);
    assert_eq!(scripted.motors.len(), 8);
    assert_eq!(scripted.motors[7].label, "H");
    assert_eq!(
        rig.service
            .frame(Some(17), Some(1), Some(0))
            .unwrap()
            .motors
            .len(),
        1
    );
    assert_eq!(
        rig.service
            .frame(Some(17), Some(1), Some(99))
            .unwrap()
            .motors
            .len(),
        12
    );
}

#[tokio::test]
async fn spin_parameters_are_written_as_fractions() {
    let rig = rig(false).await;
    rig.service.set_spin(SpinParam::Arm, 10.0).await.unwrap();
    rig.service.set_spin(SpinParam::Min, 15.0).await.unwrap();
    assert!(rig.service.set_spin(SpinParam::Min, 50.0).await.is_err());
    let writes = rig.writes.lock().unwrap().clone();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].0, "MOT_SPIN_ARM");
    assert!((writes[0].1 - 0.10).abs() < 1e-9);
    assert_eq!(writes[1].0, "MOT_SPIN_MIN");
    assert!((writes[1].1 - 0.15).abs() < 1e-9);
}
