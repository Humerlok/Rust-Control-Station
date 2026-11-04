//! Calibration flows against a fake vehicle over UDP loopback.
// Inspecting the legacy REQUEST_DATA_STREAM message the service sends.
#![allow(deprecated)]

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_link::{
    CalState, CalibrationKind, CalibrationService, ConnectionConfig, LinkManager, ParamWriter,
};
use control_station_mavlink::{
    dialect::{
        MagCalStatus, MavAutopilot, MavCmd, MavModeFlag, MavResult, MavState, MavType,
        COMMAND_ACK_DATA, COMMAND_LONG_DATA, HEARTBEAT_DATA, MAG_CAL_PROGRESS_DATA,
        MAG_CAL_REPORT_DATA, RC_CHANNELS_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion, Parser,
};
use tokio::net::UdpSocket;

struct Vehicle {
    socket: Arc<UdpSocket>,
    target: String,
    sequence: Arc<AtomicU8>,
    received: Arc<Mutex<Vec<MavMessage>>>,
    armed: Arc<AtomicBool>,
}

impl Vehicle {
    async fn start(gcs_port: u16, armed: bool) -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let vehicle = Self {
            socket: socket.clone(),
            target: format!("127.0.0.1:{gcs_port}"),
            sequence: Arc::new(AtomicU8::new(0)),
            received: Arc::new(Mutex::new(Vec::new())),
            armed: Arc::new(AtomicBool::new(armed)),
        };
        let (tx, flag, seq, target) = (
            socket.clone(),
            vehicle.armed.clone(),
            vehicle.sequence.clone(),
            vehicle.target.clone(),
        );
        tokio::spawn(async move {
            loop {
                let message = MavMessage::HEARTBEAT(HEARTBEAT_DATA {
                    custom_mode: 0,
                    mavtype: MavType::MAV_TYPE_QUADROTOR,
                    autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
                    base_mode: if flag.load(Ordering::Relaxed) {
                        MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
                    } else {
                        MavModeFlag::empty()
                    },
                    system_status: MavState::MAV_STATE_STANDBY,
                    mavlink_version: 3,
                });
                let frame = encode(
                    MavHeader {
                        system_id: 1,
                        component_id: 1,
                        sequence: seq.fetch_add(1, Ordering::Relaxed),
                    },
                    &message,
                    MavlinkVersion::V2,
                );
                tx.send_to(&frame, &target).await.ok();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        let (rx, log) = (socket, vehicle.received.clone());
        tokio::spawn(async move {
            let mut parser = Parser::new();
            let mut buf = [0u8; 2048];
            while let Ok(n) = rx.recv(&mut buf).await {
                for packet in parser.push(&buf[..n]) {
                    log.lock().unwrap().push(packet.message);
                }
            }
        });
        vehicle
    }

    async fn send(&self, message: MavMessage) {
        let frame = encode(
            MavHeader {
                system_id: 1,
                component_id: 1,
                sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            },
            &message,
            MavlinkVersion::V2,
        );
        self.socket.send_to(&frame, &self.target).await.unwrap();
    }

    async fn ack(&self, command: MavCmd, result: MavResult) {
        self.send(MavMessage::COMMAND_ACK(COMMAND_ACK_DATA {
            command,
            result,
            ..COMMAND_ACK_DATA::default()
        }))
        .await;
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
    link: LinkManager,
    service: CalibrationService,
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
    let service = CalibrationService::new(link.clone(), writer);
    let runner = service.clone();
    tokio::spawn(async move { runner.run().await });
    Rig {
        link,
        service,
        vehicle,
        writes,
    }
}

#[tokio::test]
async fn accelerometer_full_calibration_flow() {
    let r = rig(false).await;
    let message = r.service.start(CalibrationKind::AccelFull).await.unwrap();
    assert!(message.contains("started"));
    wait_until("preflight calibration", || {
        r.vehicle
            .command_longs(MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION)
            .iter()
            .any(|c| c.param5 == 1.0)
    })
    .await;

    r.vehicle
        .send(MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
            command: MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS,
            param1: 2.0,
            ..COMMAND_LONG_DATA::default()
        }))
        .await;
    wait_until("waiting for the left position", || {
        let s = r.service.status();
        s.state == CalState::Waiting && s.accel_position.is_some_and(|p| p.name == "Left")
    })
    .await;

    r.service.accel_confirm().unwrap();
    wait_until("confirmation reaches the vehicle", || {
        r.vehicle
            .command_longs(MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS)
            .iter()
            .any(|c| c.param1 == 2.0)
    })
    .await;
    assert_eq!(r.service.status().state, CalState::Running);

    r.vehicle
        .send(MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
            command: MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS,
            param1: 16_777_215.0,
            ..COMMAND_LONG_DATA::default()
        }))
        .await;
    wait_until("success", || {
        r.service.status().state == CalState::Succeeded
    })
    .await;
}

#[tokio::test]
async fn compass_calibration_reports_progress_and_success() {
    let r = rig(false).await;
    let service = r.service.clone();
    let starting = tokio::spawn(async move { service.start(CalibrationKind::Compass).await });
    wait_until("start command", || {
        !r.vehicle
            .command_longs(MavCmd::MAV_CMD_DO_START_MAG_CAL)
            .is_empty()
    })
    .await;
    let start = r.vehicle.command_longs(MavCmd::MAV_CMD_DO_START_MAG_CAL)[0].clone();
    assert_eq!(
        (start.param2, start.param3),
        (1.0, 1.0),
        "retry and autosave"
    );
    r.vehicle
        .ack(
            MavCmd::MAV_CMD_DO_START_MAG_CAL,
            MavResult::MAV_RESULT_ACCEPTED,
        )
        .await;
    starting.await.unwrap().unwrap();

    r.vehicle
        .send(MavMessage::MAG_CAL_PROGRESS(MAG_CAL_PROGRESS_DATA {
            compass_id: 0,
            cal_mask: 1,
            cal_status: MagCalStatus::MAG_CAL_RUNNING_STEP_ONE,
            completion_pct: 42,
            ..MAG_CAL_PROGRESS_DATA::default()
        }))
        .await;
    wait_until("progress", || {
        r.service
            .status()
            .compass
            .first()
            .is_some_and(|c| c.completion_pct == 42)
    })
    .await;

    r.vehicle
        .send(MavMessage::MAG_CAL_REPORT(MAG_CAL_REPORT_DATA {
            compass_id: 0,
            cal_mask: 1,
            cal_status: MagCalStatus::MAG_CAL_SUCCESS,
            autosaved: 1,
            fitness: 6.0,
            ..MAG_CAL_REPORT_DATA::default()
        }))
        .await;
    wait_until("success", || {
        r.service.status().state == CalState::Succeeded
    })
    .await;
    assert_eq!(r.service.status().compass[0].fitness, Some(6.0));
}

#[tokio::test]
async fn gyro_calibration_waits_for_the_ack() {
    let r = rig(false).await;
    let service = r.service.clone();
    let running = tokio::spawn(async move { service.start(CalibrationKind::Gyro).await });
    wait_until("gyro command", || {
        r.vehicle
            .command_longs(MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION)
            .iter()
            .any(|c| c.param1 == 1.0)
    })
    .await;
    // Longer than the normal 1 s ACK timeout: a resend would restart the calibration.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    r.vehicle
        .ack(
            MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION,
            MavResult::MAV_RESULT_ACCEPTED,
        )
        .await;
    running.await.unwrap().unwrap();
    assert_eq!(r.service.status().state, CalState::Succeeded);
    assert_eq!(
        r.vehicle
            .command_longs(MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION)
            .len(),
        1,
        "the command must not be resent"
    );
}

#[tokio::test]
async fn failed_level_calibration_reports_the_denial() {
    let r = rig(false).await;
    let service = r.service.clone();
    let running = tokio::spawn(async move { service.start(CalibrationKind::AccelLevel).await });
    wait_until("level command", || {
        r.vehicle
            .command_longs(MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION)
            .iter()
            .any(|c| c.param5 == 2.0)
    })
    .await;
    r.vehicle
        .ack(
            MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION,
            MavResult::MAV_RESULT_FAILED,
        )
        .await;
    assert!(running.await.unwrap().is_err());
    let status = r.service.status();
    assert_eq!(status.state, CalState::Failed);
    assert!(status.message.contains("failed"));
}

#[tokio::test]
async fn armed_vehicle_cannot_be_calibrated() {
    let r = rig(true).await;
    wait_until("armed telemetry", || r.link.telemetry().armed).await;
    for kind in [
        CalibrationKind::AccelFull,
        CalibrationKind::Compass,
        CalibrationKind::Esc,
    ] {
        let error = r.service.start(kind).await.unwrap_err().to_string();
        assert!(error.contains("armed"), "{kind:?}: {error}");
    }
    assert_eq!(r.service.status().state, CalState::Idle);
    assert!(
        r.writes.lock().unwrap().is_empty(),
        "ESC_CALIBRATION must not be written"
    );
}

#[tokio::test]
async fn esc_calibration_sets_the_parameter() {
    let r = rig(false).await;
    let message = r.service.start(CalibrationKind::Esc).await.unwrap();
    assert!(message.contains("ESC_CALIBRATION"));
    assert_eq!(
        r.writes.lock().unwrap().as_slice(),
        [("ESC_CALIBRATION".to_owned(), 3.0)]
    );
    assert_eq!(r.service.status().state, CalState::Succeeded);
}

#[tokio::test]
async fn radio_calibration_writes_min_max_trim() {
    let r = rig(false).await;
    r.service.start(CalibrationKind::Radio).await.unwrap();
    wait_until("rc stream request", || {
        r.vehicle
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|m| matches!(m, MavMessage::REQUEST_DATA_STREAM(s) if s.req_message_rate == 10))
    })
    .await;
    let rc = |ch1: u16, ch3: u16| {
        MavMessage::RC_CHANNELS(RC_CHANNELS_DATA {
            chan1_raw: ch1,
            chan2_raw: 1500,
            chan3_raw: ch3,
            chan4_raw: 1500,
            chan5_raw: u16::MAX,
            chan6_raw: u16::MAX,
            chan7_raw: u16::MAX,
            chan8_raw: u16::MAX,
            chan9_raw: u16::MAX,
            chan10_raw: u16::MAX,
            chan11_raw: u16::MAX,
            chan12_raw: u16::MAX,
            chan13_raw: u16::MAX,
            chan14_raw: u16::MAX,
            chan15_raw: u16::MAX,
            chan16_raw: u16::MAX,
            ..RC_CHANNELS_DATA::default()
        })
    };
    for (ch1, ch3) in [(1500, 1100), (1100, 1900), (1900, 1500), (1505, 1100)] {
        r.vehicle.send(rc(ch1, ch3)).await;
    }
    wait_until("radio extremes", || {
        let s = r.service.status();
        s.radio
            .first()
            .is_some_and(|c| c.min == 1100 && c.max == 1900 && c.value == 1505)
    })
    .await;

    let message = r.service.radio_finish().await.unwrap();
    assert!(message.contains("channels"));
    let writes = r.writes.lock().unwrap().clone();
    let get = |name: &str| writes.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
    assert_eq!(get("RC1_MIN"), Some(1100.0));
    assert_eq!(get("RC1_MAX"), Some(1900.0));
    assert_eq!(get("RC1_TRIM"), Some(1505.0));
    assert_eq!(get("RC3_TRIM"), Some(1100.0));
    assert_eq!(get("RC5_MIN"), None, "unconnected channels are not written");
    assert_eq!(r.service.status().state, CalState::Succeeded);
}

#[tokio::test]
async fn a_second_calibration_is_refused_while_one_runs() {
    let r = rig(false).await;
    r.service.start(CalibrationKind::Radio).await.unwrap();
    let error = r
        .service
        .start(CalibrationKind::AccelFull)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("already in progress"), "{error}");
    r.service.cancel().await.unwrap();
    assert_eq!(r.service.status().state, CalState::Idle);
}

#[tokio::test]
async fn disconnecting_resets_the_calibration() {
    let r = rig(false).await;
    r.service.start(CalibrationKind::Radio).await.unwrap();
    r.link.disconnect();
    wait_until("reset", || r.service.status().state == CalState::Idle).await;
}
