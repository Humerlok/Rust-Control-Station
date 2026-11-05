//! DroneCAN over the MAVLink CAN forward, against a fake autopilot that relays a fake CAN node.
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use control_station_dronecan::{
    transfer::{encode_transfer, CanFrame, Reassembler, Transfer, TransferKind},
    Catalog, Side, Value,
};
use control_station_link::{
    ConnectionConfig, DroneCanService, DroneCanTiming, LinkManager, NodeParamValue,
};
use control_station_mavlink::{
    dialect::{
        MavAutopilot, MavCmd, MavModeFlag, MavState, MavType, CAN_FRAME_DATA, COMMAND_LONG_DATA,
        HEARTBEAT_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion, Parser,
};
use tokio::net::UdpSocket;

const NODE: u8 = 10;
const EXTENDED: u32 = 0x8000_0000;

fn catalog() -> &'static Catalog {
    Catalog::builtin()
}

fn index(name: &str) -> usize {
    catalog()
        .index_of(name)
        .unwrap_or_else(|| panic!("type {name}"))
}

/// Behaviour switches of the fake node.
#[derive(Default)]
struct NodeConfig {
    /// Drop this many GetSet requests before answering (to exercise the retries).
    drop_get_set: AtomicU32,
    /// Stop broadcasting NodeStatus.
    silent: AtomicBool,
    /// Which CAN bus (0-based) the node sits on.
    bus: AtomicU8,
}

/// Name, value, default value and (min, max) of a parameter of the fake node.
type FakeParam = (String, Value, Value, Option<(f64, f64)>);

struct Node {
    config: Arc<NodeConfig>,
    params: Mutex<Vec<FakeParam>>,
    reassembler: Mutex<Reassembler>,
    saved: AtomicBool,
    restarted: AtomicBool,
    seen_gcs_status: AtomicBool,
}

struct Vehicle {
    socket: Arc<UdpSocket>,
    target: String,
    sequence: Arc<AtomicU8>,
    received: Arc<Mutex<Vec<MavMessage>>>,
    node: Arc<Node>,
}

fn header(sequence: &AtomicU8) -> MavHeader {
    MavHeader {
        system_id: 1,
        component_id: 1,
        sequence: sequence.fetch_add(1, Ordering::Relaxed),
    }
}

fn status_value(health: u64, mode: u64) -> Value {
    Value::strukt([
        ("uptime_sec", Value::Uint(42)),
        ("health", Value::Uint(health)),
        ("mode", Value::Uint(mode)),
        ("sub_mode", Value::Uint(0)),
        ("vendor_specific_status_code", Value::Uint(0x0102)),
    ])
}

impl Node {
    fn new(config: Arc<NodeConfig>) -> Self {
        let int = |v| Value::union("integer_value", Value::Int(v));
        let real = |v| Value::union("real_value", Value::Float(v));
        Self {
            config,
            params: Mutex::new(vec![
                (
                    "CAN_BAUDRATE".into(),
                    int(1_000_000),
                    int(1_000_000),
                    Some((10_000.0, 1_000_000.0)),
                ),
                ("GPS_TYPE".into(), int(2), int(1), Some((0.0, 20.0))),
                ("OFFSET_X".into(), real(0.5), real(0.0), None),
            ]),
            reassembler: Mutex::new(Reassembler::new()),
            saved: AtomicBool::new(false),
            restarted: AtomicBool::new(false),
            seen_gcs_status: AtomicBool::new(false),
        }
    }

    fn transfer(&self, kind: TransferKind, transfer_id: u8, payload: Vec<u8>) -> Transfer {
        Transfer {
            priority: 20,
            source: NODE,
            kind,
            transfer_id,
            payload,
        }
    }

    fn broadcast_status(&self, id: u8) -> Vec<CanFrame> {
        let ty = index("uavcan.protocol.NodeStatus");
        let payload = catalog()
            .encode(ty, Side::Message, &status_value(0, 0), true)
            .unwrap();
        let def = &catalog().types[ty];
        encode_transfer(
            &self.transfer(TransferKind::Message { type_id: 341 }, id, payload),
            def.signature,
            false,
            0,
        )
    }

    fn log_frames(&self) -> Vec<CanFrame> {
        let ty = index("uavcan.protocol.debug.LogMessage");
        let value = Value::strukt([
            ("level", Value::strukt([("value", Value::Uint(2))])),
            ("source", Value::text("gps")),
            ("text", Value::text("no fix yet")),
        ]);
        let payload = catalog().encode(ty, Side::Message, &value, true).unwrap();
        let def = &catalog().types[ty];
        encode_transfer(
            &self.transfer(
                TransferKind::Message {
                    type_id: def.id.unwrap(),
                },
                0,
                payload,
            ),
            def.signature,
            false,
            0,
        )
    }

    /// Reacts to a frame from the ground station; returns the frames to send back.
    fn on_frame(&self, frame: &CanFrame) -> Vec<CanFrame> {
        let signature_of = |kind: &TransferKind| match kind {
            TransferKind::Message { type_id } => catalog().message(*type_id).map(|t| t.signature),
            TransferKind::Request { service_id, .. }
            | TransferKind::Response { service_id, .. } => catalog()
                .service(u16::from(*service_id))
                .map(|t| t.signature),
        };
        let Some(transfer) =
            self.reassembler
                .lock()
                .unwrap()
                .push(frame, Instant::now(), signature_of)
        else {
            return vec![];
        };
        match transfer.kind {
            TransferKind::Message { type_id: 341 } if transfer.source == 127 => {
                self.seen_gcs_status.store(true, Ordering::Relaxed);
                vec![]
            }
            TransferKind::Request {
                service_id,
                destination,
            } if destination == NODE => {
                let def = catalog()
                    .service(u16::from(service_id))
                    .expect("known service");
                let ty = catalog().index_of(&def.full_name).unwrap();
                let request = catalog()
                    .decode(ty, Side::Request, &transfer.payload, true)
                    .unwrap();
                let Some(response) = self.respond(&def.full_name, &request) else {
                    return vec![];
                };
                let payload = catalog()
                    .encode(ty, Side::Response, &response, true)
                    .unwrap();
                encode_transfer(
                    &self.transfer(
                        TransferKind::Response {
                            service_id,
                            destination: transfer.source,
                        },
                        transfer.transfer_id,
                        payload,
                    ),
                    def.signature,
                    false,
                    0,
                )
            }
            _ => vec![],
        }
    }

    fn param_response(&self, index: usize) -> Value {
        let params = self.params.lock().unwrap();
        let empty = Value::union("empty", Value::Struct(vec![]));
        match params.get(index) {
            Some((name, value, default, limits)) => {
                let numeric = |v: f64| Value::union("real_value", Value::Float(v));
                Value::strukt([
                    ("value", value.clone()),
                    ("default_value", default.clone()),
                    ("max_value", limits.map_or(empty.clone(), |l| numeric(l.1))),
                    ("min_value", limits.map_or(empty.clone(), |l| numeric(l.0))),
                    ("name", Value::text(name)),
                ])
            }
            None => Value::strukt([("value", empty.clone()), ("name", Value::text(""))]),
        }
    }

    fn respond(&self, service: &str, request: &Value) -> Option<Value> {
        match service {
            "uavcan.protocol.GetNodeInfo" => Some(Value::strukt([
                ("status", status_value(0, 0)),
                (
                    "software_version",
                    Value::strukt([
                        ("major", Value::Uint(1)),
                        ("minor", Value::Uint(2)),
                        ("optional_field_flags", Value::Uint(3)),
                        ("vcs_commit", Value::Uint(0xABCD_1234)),
                        ("image_crc", Value::Uint(0x1122_3344_5566_7788)),
                    ]),
                ),
                (
                    "hardware_version",
                    Value::strukt([
                        ("major", Value::Uint(3)),
                        ("minor", Value::Uint(4)),
                        (
                            "unique_id",
                            Value::Array((1..=16).map(Value::Uint).collect()),
                        ),
                    ]),
                ),
                ("name", Value::text("com.example.gps")),
            ])),
            "uavcan.protocol.param.GetSet" => {
                // Lose some requests on purpose.
                if self.config.drop_get_set.load(Ordering::Relaxed) > 0 {
                    self.config.drop_get_set.fetch_sub(1, Ordering::Relaxed);
                    return None;
                }
                let name = request
                    .get("name")
                    .and_then(Value::as_text)
                    .unwrap_or_default();
                if name.is_empty() {
                    let index = request.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    return Some(self.param_response(index));
                }
                let position = self.params.lock().unwrap().iter().position(|p| p.0 == name);
                match position {
                    Some(position) => {
                        let new = request.get("value").cloned().unwrap();
                        if new.variant().is_some_and(|(v, _)| v != "empty") {
                            self.params.lock().unwrap()[position].1 = new;
                        }
                        Some(self.param_response(position))
                    }
                    None => Some(self.param_response(usize::MAX)),
                }
            }
            "uavcan.protocol.param.ExecuteOpcode" => {
                self.saved.store(true, Ordering::Relaxed);
                Some(Value::strukt([
                    ("argument", Value::Int(0)),
                    ("ok", Value::Bool(true)),
                ]))
            }
            "uavcan.protocol.RestartNode" => {
                assert_eq!(
                    request.get("magic_number").and_then(Value::as_u64),
                    Some(0xACCE_551B1E)
                );
                self.restarted.store(true, Ordering::Relaxed);
                Some(Value::strukt([("ok", Value::Bool(true))]))
            }
            _ => None,
        }
    }
}

impl Vehicle {
    fn wrap(&self, frame: &CanFrame, bus: u8) -> MavMessage {
        let mut data = [0u8; 8];
        data[..frame.data.len()].copy_from_slice(&frame.data);
        MavMessage::CAN_FRAME(CAN_FRAME_DATA {
            id: frame.id | EXTENDED,
            target_system: 255,
            target_component: 190,
            bus,
            len: frame.data.len() as u8,
            data,
        })
    }

    async fn start(gcs_port: u16, config: Arc<NodeConfig>) -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let vehicle = Self {
            socket: socket.clone(),
            target: format!("127.0.0.1:{gcs_port}"),
            sequence: Arc::new(AtomicU8::new(0)),
            received: Arc::new(Mutex::new(Vec::new())),
            node: Arc::new(Node::new(config)),
        };
        // Heartbeat.
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
                    base_mode: MavModeFlag::empty(),
                    system_status: MavState::MAV_STATE_STANDBY,
                    mavlink_version: 3,
                });
                tx.send_to(&encode(header(&seq), &message, MavlinkVersion::V2), &target)
                    .await
                    .ok();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        // The relayed node: NodeStatus every 50 ms, and one log message.
        let (v_socket, v_seq, v_target, node) = (
            socket.clone(),
            vehicle.sequence.clone(),
            vehicle.target.clone(),
            vehicle.node.clone(),
        );
        let relay = vehicle.relayer();
        tokio::spawn(async move {
            let mut id = 0u8;
            let mut logged = false;
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if node.config.silent.load(Ordering::Relaxed) {
                    continue;
                }
                let bus = node.config.bus.load(Ordering::Relaxed);
                for frame in node.broadcast_status(id) {
                    v_socket
                        .send_to(
                            &encode(
                                header(&v_seq),
                                &relay.wrap_with(&frame, bus),
                                MavlinkVersion::V2,
                            ),
                            &v_target,
                        )
                        .await
                        .ok();
                }
                if !logged && id > 3 {
                    logged = true;
                    for frame in node.log_frames() {
                        v_socket
                            .send_to(
                                &encode(
                                    header(&v_seq),
                                    &relay.wrap_with(&frame, bus),
                                    MavlinkVersion::V2,
                                ),
                                &v_target,
                            )
                            .await
                            .ok();
                    }
                }
                id = (id + 1) & 0x1F;
            }
        });
        // What the ground station sends.
        let (rx, log, seq, target, node, relay) = (
            socket,
            vehicle.received.clone(),
            vehicle.sequence.clone(),
            vehicle.target.clone(),
            vehicle.node.clone(),
            vehicle.relayer(),
        );
        tokio::spawn(async move {
            let mut parser = Parser::new();
            let mut buf = [0u8; 2048];
            while let Ok(n) = rx.recv(&mut buf).await {
                for packet in parser.push(&buf[..n]) {
                    if let MavMessage::CAN_FRAME(f) = &packet.message {
                        if f.id & EXTENDED != 0 && f.bus == node.config.bus.load(Ordering::Relaxed)
                        {
                            let frame = CanFrame {
                                id: f.id & 0x1FFF_FFFF,
                                data: f.data[..usize::from(f.len)].to_vec(),
                            };
                            for reply in node.on_frame(&frame) {
                                let bus = node.config.bus.load(Ordering::Relaxed);
                                rx.send_to(
                                    &encode(
                                        header(&seq),
                                        &relay.wrap_with(&reply, bus),
                                        MavlinkVersion::V2,
                                    ),
                                    &target,
                                )
                                .await
                                .ok();
                            }
                        }
                    }
                    log.lock().unwrap().push(packet.message);
                }
            }
        });
        vehicle
    }

    fn relayer(&self) -> Relayer {
        Relayer
    }

    fn can_forwards(&self) -> Vec<COMMAND_LONG_DATA> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MavMessage::COMMAND_LONG(c) if c.command == MavCmd::MAV_CMD_CAN_FORWARD => {
                    Some(c.clone())
                }
                _ => None,
            })
            .collect()
    }

    async fn send_raw(&self, frame: &CanFrame, bus: u8) {
        let message = self.wrap(frame, bus);
        self.socket
            .send_to(
                &encode(header(&self.sequence), &message, MavlinkVersion::V2),
                &self.target,
            )
            .await
            .unwrap();
    }
}

struct Relayer;

impl Relayer {
    fn wrap_with(&self, frame: &CanFrame, bus: u8) -> MavMessage {
        let mut data = [0u8; 8];
        data[..frame.data.len()].copy_from_slice(&frame.data);
        MavMessage::CAN_FRAME(CAN_FRAME_DATA {
            id: frame.id | EXTENDED,
            target_system: 255,
            target_component: 190,
            bus,
            len: frame.data.len() as u8,
            data,
        })
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

fn fast() -> DroneCanTiming {
    DroneCanTiming {
        forward_interval: Duration::from_millis(100),
        status_interval: Duration::from_millis(100),
        info_interval: Duration::from_millis(200),
        request_timeout: Duration::from_millis(300),
        request_attempts: 4,
        offline_after: Duration::from_millis(500),
        tick: Duration::from_millis(20),
    }
}

struct Rig {
    service: DroneCanService,
    vehicle: Vehicle,
    config: Arc<NodeConfig>,
}

async fn rig() -> Rig {
    let link = LinkManager::new();
    link.connect(ConnectionConfig::Udp { port: 0 })
        .await
        .unwrap();
    let config = Arc::new(NodeConfig::default());
    let vehicle = Vehicle::start(link.local_port().unwrap(), config.clone()).await;
    wait_until("vehicle", || link.telemetry().connected).await;
    let service = DroneCanService::with_timing(link, fast());
    let runner = service.clone();
    tokio::spawn(async move { runner.run().await });
    Rig {
        service,
        vehicle,
        config,
    }
}

async fn started(bus: u8) -> Rig {
    let rig = rig().await;
    rig.config.bus.store(bus - 1, Ordering::Relaxed);
    rig.service.start(bus).unwrap();
    wait_until("the node info", || {
        rig.service
            .status()
            .nodes
            .first()
            .is_some_and(|n| n.name.is_some())
    })
    .await;
    rig
}

#[tokio::test]
async fn nodes_appear_with_status_info_and_logs() {
    let rig = started(1).await;
    let state = rig.service.status();
    assert!(state.active);
    assert_eq!(state.bus, Some(1));
    let node = &state.nodes[0];
    assert_eq!(node.id, NODE);
    assert_eq!(
        (node.health.as_str(), node.mode.as_str()),
        ("OK", "OPERATIONAL")
    );
    assert_eq!(node.name.as_deref(), Some("com.example.gps"));
    assert_eq!(node.sw_version.as_deref(), Some("1.2 (abcd1234)"));
    assert_eq!(node.sw_crc.as_deref(), Some("1122334455667788"));
    assert_eq!(node.hw_version.as_deref(), Some("3.4"));
    assert_eq!(
        node.unique_id.as_deref(),
        Some("0102030405060708090A0B0C0D0E0F10")
    );
    assert_eq!(node.vendor_status, 0x0102);
    assert!(node.online);

    // The ground station renews CAN forward on its bus and announces itself on the CAN bus.
    wait_until("repeated CAN forward", || {
        rig.vehicle.can_forwards().len() >= 3
    })
    .await;
    assert!(rig.vehicle.can_forwards().iter().all(|c| c.param1 == 1.0));
    wait_until("the GCS NodeStatus", || {
        rig.vehicle.node.seen_gcs_status.load(Ordering::Relaxed)
    })
    .await;

    wait_until("the log message", || !rig.service.status().logs.is_empty()).await;
    let log = &rig.service.status().logs[0];
    assert_eq!(
        (
            log.node,
            log.level.as_str(),
            log.source.as_str(),
            log.text.as_str()
        ),
        (NODE, "WARNING", "gps", "no fix yet")
    );
}

#[tokio::test]
async fn parameters_can_be_listed_set_saved_and_the_node_restarted() {
    let rig = started(2).await;
    let params = rig.service.list_params(NODE).await.unwrap();
    assert_eq!(
        params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["CAN_BAUDRATE", "GPS_TYPE", "OFFSET_X"]
    );
    assert_eq!(params[1].value, NodeParamValue::Integer(2));
    assert_eq!(params[1].default, Some(NodeParamValue::Integer(1)));
    assert_eq!((params[1].min, params[1].max), (Some(0.0), Some(20.0)));
    assert_eq!(params[2].value, NodeParamValue::Real(0.5));
    assert_eq!((params[2].min, params[2].max), (None, None));

    let echoed = rig
        .service
        .set_param(NODE, "GPS_TYPE", NodeParamValue::Integer(9))
        .await
        .unwrap();
    assert_eq!(echoed.value, NodeParamValue::Integer(9));
    let echoed = rig
        .service
        .set_param(NODE, "OFFSET_X", NodeParamValue::Real(-1.5))
        .await
        .unwrap();
    assert_eq!(echoed.value, NodeParamValue::Real(-1.5));
    let error = rig
        .service
        .set_param(NODE, "NOPE", NodeParamValue::Integer(1))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("NOPE"), "{error}");

    rig.service.save_params(NODE).await.unwrap();
    assert!(rig.vehicle.node.saved.load(Ordering::Relaxed));
    let message = rig.service.restart_node(NODE).await.unwrap();
    assert!(message.contains("restarting"), "{message}");
    assert!(rig.vehicle.node.restarted.load(Ordering::Relaxed));
    assert_eq!(
        rig.vehicle.can_forwards().last().map(|c| c.param1),
        Some(2.0),
        "bus 2"
    );
}

#[tokio::test]
async fn lost_requests_are_sent_again() {
    let rig = started(1).await;
    rig.config.drop_get_set.store(2, Ordering::Relaxed);
    let params = rig.service.list_params(NODE).await.unwrap();
    assert_eq!(
        params.len(),
        3,
        "the first two requests were dropped and retried"
    );
}

#[tokio::test]
async fn a_dead_node_is_an_error_not_a_hang() {
    let rig = started(1).await;
    let error = rig.service.list_params(42).await.unwrap_err();
    assert!(error.to_string().contains("did not answer"), "{error}");
    // The node is free again afterwards.
    assert!(rig.service.list_params(NODE).await.is_ok());
}

#[tokio::test]
async fn operations_need_a_running_relay_and_stop_turns_it_off() {
    let rig = rig().await;
    assert!(rig.service.list_params(NODE).await.is_err());
    assert!(rig.service.start(3).is_err(), "only buses 1 and 2");
    rig.service.start(1).unwrap();
    assert!(rig.service.start(1).is_err(), "already running");
    wait_until("a node", || !rig.service.status().nodes.is_empty()).await;
    rig.service.stop().unwrap();
    assert!(!rig.service.status().active);
    assert!(rig.service.status().nodes.is_empty());
    wait_until("the stop command", || {
        rig.vehicle
            .can_forwards()
            .last()
            .is_some_and(|c| c.param1 == 0.0)
    })
    .await;
}

#[tokio::test]
async fn frames_from_the_other_bus_are_ignored() {
    let rig = rig().await;
    rig.config.silent.store(true, Ordering::Relaxed);
    rig.service.start(1).unwrap();
    // A NodeStatus relayed from CAN2 (bus index 1) while CAN1 is in use.
    for frame in rig.vehicle.node.broadcast_status(0) {
        rig.vehicle.send_raw(&frame, 1).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(rig.service.status().nodes.is_empty());
    // The same frame on the right bus is accepted.
    for frame in rig.vehicle.node.broadcast_status(1) {
        rig.vehicle.send_raw(&frame, 0).await;
    }
    wait_until("the node", || rig.service.status().nodes.len() == 1).await;
}

#[tokio::test]
async fn a_node_that_goes_quiet_is_shown_offline() {
    let rig = started(1).await;
    rig.config.silent.store(true, Ordering::Relaxed);
    wait_until("offline", || {
        rig.service
            .status()
            .nodes
            .first()
            .is_some_and(|n| !n.online)
    })
    .await;
    // It comes back by itself.
    rig.config.silent.store(false, Ordering::Relaxed);
    wait_until("online again", || {
        rig.service.status().nodes.first().is_some_and(|n| n.online)
    })
    .await;
}
