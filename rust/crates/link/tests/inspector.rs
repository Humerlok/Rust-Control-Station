//! The inspector against a fake vehicle over UDP loopback.
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use control_station_link::{ConnectionConfig, InspectorService, InspectorSnapshot, LinkManager};
use control_station_mavlink::{
    dialect::{
        MavAutopilot, MavModeFlag, MavState, MavType, ATTITUDE_DATA, HEARTBEAT_DATA,
        STATUSTEXT_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion,
};
use tokio::{net::UdpSocket, sync::broadcast, time::timeout};

const ATTITUDE_ID: u32 = 30;
const ATTITUDE_KEY: &str = "1/1/30/roll";

/// Sends ATTITUDE at 20 Hz and HEARTBEAT at 2 Hz until dropped or stopped.
struct Vehicle {
    sending: Arc<AtomicBool>,
}

impl Vehicle {
    async fn start(gcs_port: u16) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sending = Arc::new(AtomicBool::new(true));
        let flag = sending.clone();
        let sequence = AtomicU8::new(0);
        let target = format!("127.0.0.1:{gcs_port}");
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            let mut n: u32 = 0;
            loop {
                tick.tick().await;
                if !flag.load(Ordering::Relaxed) {
                    continue;
                }
                n += 1;
                let mut messages = vec![MavMessage::ATTITUDE(ATTITUDE_DATA {
                    time_boot_ms: n,
                    roll: 0.5,
                    ..ATTITUDE_DATA::default()
                })];
                if n % 10 == 1 {
                    messages.push(MavMessage::HEARTBEAT(HEARTBEAT_DATA {
                        custom_mode: 5,
                        mavtype: MavType::MAV_TYPE_QUADROTOR,
                        autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
                        base_mode: MavModeFlag::empty(),
                        system_status: MavState::MAV_STATE_ACTIVE,
                        mavlink_version: 3,
                    }));
                    messages.push(MavMessage::STATUSTEXT(STATUSTEXT_DATA {
                        text: "Hello inspector".into(),
                        ..STATUSTEXT_DATA::default()
                    }));
                }
                for message in messages {
                    let frame = encode(
                        MavHeader {
                            system_id: 1,
                            component_id: 1,
                            sequence: sequence.fetch_add(1, Ordering::Relaxed),
                        },
                        &message,
                        MavlinkVersion::V2,
                    );
                    socket.send_to(&frame, &target).await.ok();
                }
            }
        });
        Self { sending }
    }

    fn pause(&self) {
        self.sending.store(false, Ordering::Relaxed);
    }
}

struct Rig {
    link: LinkManager,
    service: InspectorService,
    snapshots: broadcast::Receiver<InspectorSnapshot>,
    vehicle: Vehicle,
}

async fn rig() -> Rig {
    let link = LinkManager::new();
    link.connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let service = InspectorService::new(link.clone());
    let snapshots = service.subscribe();
    let runner = service.clone();
    tokio::spawn(async move { runner.run().await });
    let vehicle = Vehicle::start(link.local_port().unwrap()).await;
    Rig {
        link,
        service,
        snapshots,
        vehicle,
    }
}

async fn next(snapshots: &mut broadcast::Receiver<InspectorSnapshot>) -> InspectorSnapshot {
    loop {
        match timeout(Duration::from_secs(3), snapshots.recv()).await {
            Ok(Ok(snapshot)) => return snapshot,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            other => panic!("no snapshot: {other:?}"),
        }
    }
}

fn attitude(snapshot: &InspectorSnapshot) -> Option<&control_station_link::InspectorMessage> {
    snapshot
        .messages
        .iter()
        .find(|m| (m.system_id, m.component_id, m.message_id) == (1, 1, ATTITUDE_ID))
}

#[tokio::test]
async fn rates_counts_and_fields_of_a_live_vehicle() {
    let mut r = rig().await;
    r.service.set_enabled(true);
    tokio::time::sleep(Duration::from_millis(2600)).await;
    // Take the freshest snapshot.
    let mut snapshot = next(&mut r.snapshots).await;
    while let Ok(newer) = r.snapshots.try_recv() {
        snapshot = newer;
    }

    let att = attitude(&snapshot).expect("ATTITUDE listed");
    assert_eq!(att.name, "ATTITUDE");
    assert_eq!(att.component_name, "Autopilot");
    assert!((15.0..=23.0).contains(&att.hz), "ATTITUDE rate {}", att.hz);
    assert!(att.count >= 40, "count {}", att.count);
    assert!(
        att.bytes_per_second > 300.0,
        "bytes/s {}",
        att.bytes_per_second
    );
    assert!(att.age_ms < 500, "age {}", att.age_ms);
    let roll = att.fields.iter().find(|f| f.name == "roll").unwrap();
    assert_eq!(roll.value, serde_json::json!(0.5));

    let heartbeat = snapshot
        .messages
        .iter()
        .find(|m| m.message_id == 0)
        .expect("HEARTBEAT listed");
    assert!(
        (1.0..=3.0).contains(&heartbeat.hz),
        "HEARTBEAT rate {}",
        heartbeat.hz
    );
    let mode = heartbeat
        .fields
        .iter()
        .find(|f| f.name == "mavtype")
        .unwrap();
    assert_eq!(mode.value, serde_json::json!("MAV_TYPE_QUADROTOR"));

    let text = snapshot
        .messages
        .iter()
        .find(|m| m.name == "STATUSTEXT")
        .unwrap();
    let text_field = text.fields.iter().find(|f| f.name == "text").unwrap();
    assert_eq!(text_field.value, serde_json::json!("Hello inspector"));

    let order: Vec<_> = snapshot
        .messages
        .iter()
        .map(|m| (m.system_id, m.component_id, m.message_id))
        .collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted);
}

#[tokio::test]
async fn watch_streams_points_until_unwatch() {
    let mut r = rig().await;
    r.service.set_enabled(true);
    assert!(r.service.watch("not a key").is_err());
    r.service.watch(ATTITUDE_KEY).unwrap();

    let mut points = Vec::new();
    timeout(Duration::from_secs(4), async {
        while points.len() < 10 {
            let snapshot = next(&mut r.snapshots).await;
            if let Some(new) = snapshot.series.get(ATTITUDE_KEY) {
                points.extend(new.iter().copied());
            }
        }
    })
    .await
    .expect("watched points arrive");
    assert!(points.iter().all(|(_, v)| *v == 0.5));
    assert!(
        points.windows(2).all(|w| w[1].0 >= w[0].0),
        "times never go back"
    );

    r.service.unwatch(ATTITUDE_KEY);
    // Drop what was already in flight, then no snapshot may carry the key.
    tokio::time::sleep(Duration::from_millis(300)).await;
    while r.snapshots.try_recv().is_ok() {}
    for _ in 0..3 {
        let snapshot = next(&mut r.snapshots).await;
        assert!(!snapshot.series.contains_key(ATTITUDE_KEY));
    }
}

#[tokio::test]
async fn disabled_inspector_publishes_nothing() {
    let mut r = rig().await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(matches!(
        r.snapshots.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    r.service.set_enabled(true);
    assert!(next(&mut r.snapshots).await.messages.len() <= 3);
    r.service.set_enabled(false);
    tokio::time::sleep(Duration::from_millis(300)).await;
    while r.snapshots.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(matches!(
        r.snapshots.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn clear_resets_the_counters() {
    let mut r = rig().await;
    r.service.set_enabled(true);
    let before = loop {
        let snapshot = next(&mut r.snapshots).await;
        if let Some(att) = attitude(&snapshot) {
            if att.count >= 30 {
                break att.count;
            }
        }
    };
    r.service.clear();
    // Snapshots published before the clear may still be queued: drop them.
    tokio::time::sleep(Duration::from_millis(300)).await;
    while r.snapshots.try_recv().is_ok() {}
    let after = loop {
        let snapshot = next(&mut r.snapshots).await;
        if let Some(att) = attitude(&snapshot) {
            break att.count;
        }
    };
    // About 0.6 s of 20 Hz traffic since the clear, against 1.5 s+ before it.
    assert!(after < before, "count went from {before} to {after}");
    assert!(after <= 20, "count restarted from zero, got {after}");
}

#[tokio::test]
async fn disconnect_resets_everything() {
    let mut r = rig().await;
    r.service.set_enabled(true);
    timeout(Duration::from_secs(3), async {
        while attitude(&next(&mut r.snapshots).await).is_none() {}
    })
    .await
    .expect("messages seen");
    r.vehicle.pause();
    r.link.disconnect();
    tokio::time::sleep(Duration::from_millis(400)).await;
    while r.snapshots.try_recv().is_ok() {}
    let snapshot = next(&mut r.snapshots).await;
    assert!(
        snapshot.messages.is_empty(),
        "{:?}",
        snapshot.messages.len()
    );
    assert!(r.service.is_enabled(), "the inspector itself stays enabled");
}
