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
    ConnectionConfig, DroneCanService, DroneCanTiming, LinkManager, NodeParamValue, UpdatePhase,
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
    /// The vehicle reports itself armed.
    armed: AtomicBool,
    /// The `error` the node answers BeginFirmwareUpdate with (0 accepts).
    begin_error: AtomicU8,
    /// Stop asking for firmware chunks after this many (0 never stops).
    fw_stall_after: AtomicU32,
    /// Accept BeginFirmwareUpdate but never ask for the file.
    fw_never_reads: AtomicBool,
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
    /// Firmware the node has fetched so far.
    fw_received: Mutex<Vec<u8>>,
    fw_begin_count: AtomicU32,
    fw_chunks: AtomicU32,
    fw_done: AtomicBool,
    read_id: AtomicU8,
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
            fw_received: Mutex::new(Vec::new()),
            fw_begin_count: AtomicU32::new(0),
            fw_chunks: AtomicU32::new(0),
            fw_done: AtomicBool::new(false),
            read_id: AtomicU8::new(0),
        }
    }

    /// The node asks the ground station for the next piece of the firmware file.
    fn read_request(&self, offset: usize) -> Vec<CanFrame> {
        let ty = index("uavcan.protocol.file.Read");
        let request = Value::strukt([
            ("offset", Value::Uint(offset as u64)),
            ("path", Value::strukt([("path", Value::text("fw.bin"))])),
        ]);
        let payload = catalog().encode(ty, Side::Request, &request, true).unwrap();
        let def = &catalog().types[ty];
        let id = self.read_id.fetch_add(1, Ordering::Relaxed) & 0x1F;
        encode_transfer(
            &self.transfer(
                TransferKind::Request {
                    service_id: 48,
                    destination: 127,
                },
                id,
                payload,
            ),
            def.signature,
            false,
            0,
        )
    }

    /// A piece of firmware arrived: keep it and ask for the next one.
    fn on_file_data(&self, response: &Value) -> Vec<CanFrame> {
        if response.get("error.value").and_then(Value::as_i64) != Some(0) {
            return vec![]; // not found: the ground station gave up on us
        }
        let data: Vec<u8> = response
            .get("data")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect();
        let chunks = self.fw_chunks.fetch_add(1, Ordering::Relaxed) + 1;
        let mut received = self.fw_received.lock().unwrap();
        received.extend_from_slice(&data);
        if data.len() < 256 {
            self.fw_done.store(true, Ordering::Relaxed);
            return vec![];
        }
        let stall = self.config.fw_stall_after.load(Ordering::Relaxed);
        if stall != 0 && chunks >= stall {
            return vec![];
        }
        self.read_request(received.len())
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
                let mut frames = encode_transfer(
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
                );
                // An accepted update: the node starts fetching the image right away.
                if def.full_name == "uavcan.protocol.file.BeginFirmwareUpdate"
                    && self.config.begin_error.load(Ordering::Relaxed) == 0
                    && !self.config.fw_never_reads.load(Ordering::Relaxed)
                {
                    self.fw_received.lock().unwrap().clear();
                    self.fw_chunks.store(0, Ordering::Relaxed);
                    self.fw_done.store(false, Ordering::Relaxed);
                    frames.extend(self.read_request(0));
                }
                frames
            }
            TransferKind::Response {
                service_id: 48,
                destination,
            } if destination == NODE => {
                let ty = index("uavcan.protocol.file.Read");
                let response = catalog()
                    .decode(ty, Side::Response, &transfer.payload, true)
                    .unwrap();
                self.on_file_data(&response)
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
            "uavcan.protocol.file.BeginFirmwareUpdate" => {
                assert_eq!(
                    request.get("source_node_id").and_then(Value::as_u64),
                    Some(127)
                );
                assert_eq!(
                    request
                        .get("image_file_remote_path.path")
                        .and_then(Value::as_text)
                        .as_deref(),
                    Some("fw.bin")
                );
                self.fw_begin_count.fetch_add(1, Ordering::Relaxed);
                Some(Value::strukt([
                    (
                        "error",
                        Value::Uint(u64::from(self.config.begin_error.load(Ordering::Relaxed))),
                    ),
                    ("optional_error_message", Value::text("")),
                ]))
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
        let heartbeat_config = config.clone();
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
                    base_mode: if heartbeat_config.armed.load(Ordering::Relaxed) {
                        MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
                    } else {
                        MavModeFlag::empty()
                    },
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
        update_start_timeout: Duration::from_millis(800),
        update_stall_timeout: Duration::from_millis(500),
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

// ---- node firmware update ----

const DESCRIPTOR_SIGNATURE: [u8; 8] = [0xD7, 0xE4, 0xF7, 0xBA, 0xD0, 0x0F, 0x9B, 0xEE];

/// A firmware image of `len` bytes that carries an application descriptor with `crc`.
fn image(crc: u64, len: usize) -> Vec<u8> {
    let mut data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    data[64..72].copy_from_slice(&DESCRIPTOR_SIGNATURE);
    data[72..80].copy_from_slice(&crc.to_le_bytes());
    data
}

const NEW_CRC: u64 = 0xAAAA_BBBB_CCCC_DDDD;

#[tokio::test]
async fn an_update_serves_the_whole_image_to_the_node() {
    let rig = started(1).await;
    let firmware = image(NEW_CRC, 2000);
    let message = rig
        .service
        .start_update(NODE, firmware.clone(), false)
        .await
        .unwrap();
    assert!(message.contains("2000 bytes"), "{message}");
    wait_until("the transfer to finish", || {
        rig.service
            .status()
            .update
            .is_some_and(|u| u.phase == UpdatePhase::Done)
    })
    .await;
    let update = rig.service.status().update.unwrap();
    assert_eq!((update.node, update.sent, update.total), (NODE, 2000, 2000));
    assert_eq!(*rig.vehicle.node.fw_received.lock().unwrap(), firmware);
    assert_eq!(rig.vehicle.node.fw_begin_count.load(Ordering::Relaxed), 1);
    // The node is free again.
    assert!(rig.service.list_params(NODE).await.is_ok());
}

#[tokio::test]
async fn an_image_the_node_already_runs_is_not_sent_unless_forced() {
    let rig = started(1).await;
    let same = image(0x1122_3344_5566_7788, 600);
    let message = rig
        .service
        .start_update(NODE, same.clone(), false)
        .await
        .unwrap();
    assert!(message.contains("already runs"), "{message}");
    assert_eq!(rig.vehicle.node.fw_begin_count.load(Ordering::Relaxed), 0);
    assert!(rig.service.status().update.is_none());
    rig.service.start_update(NODE, same, true).await.unwrap();
    wait_until("the forced transfer", || {
        rig.vehicle.node.fw_done.load(Ordering::Relaxed)
    })
    .await;
}

#[tokio::test]
async fn updates_are_refused_in_the_cases_that_would_be_unsafe() {
    let rig = started(1).await;
    let firmware = image(NEW_CRC, 600);
    assert!(rig.service.start_update(NODE, vec![], false).await.is_err());
    assert!(rig
        .service
        .start_update(NODE, b"{}".to_vec(), false)
        .await
        .is_err());
    let unknown = rig
        .service
        .start_update(77, firmware.clone(), false)
        .await
        .unwrap_err();
    assert!(unknown.to_string().contains("not on the bus"), "{unknown}");

    // The node says no.
    rig.config.begin_error.store(1, Ordering::Relaxed);
    let refused = rig
        .service
        .start_update(NODE, firmware.clone(), false)
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("refused"), "{refused}");
    assert_eq!(
        rig.service.status().update.unwrap().phase,
        UpdatePhase::Failed
    );
    assert!(
        rig.service.list_params(NODE).await.is_ok(),
        "the node was released"
    );
    rig.config.begin_error.store(0, Ordering::Relaxed);

    // An armed vehicle never updates a node.
    rig.config.armed.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let armed = rig
        .service
        .start_update(NODE, firmware, false)
        .await
        .unwrap_err();
    assert!(armed.to_string().contains("armed"), "{armed}");
    assert_eq!(
        rig.vehicle.node.fw_begin_count.load(Ordering::Relaxed),
        1,
        "only the refused attempt reached the node"
    );
}

#[tokio::test]
async fn a_stalled_or_ignored_update_fails_by_itself() {
    let rig = started(1).await;
    rig.config.fw_stall_after.store(2, Ordering::Relaxed);
    rig.service
        .start_update(NODE, image(NEW_CRC, 3000), false)
        .await
        .unwrap();
    wait_until("the stall", || {
        rig.service
            .status()
            .update
            .is_some_and(|u| u.phase == UpdatePhase::Failed)
    })
    .await;
    let update = rig.service.status().update.unwrap();
    assert!(update.message.contains("stalled"), "{}", update.message);
    assert!(update.sent > 0 && update.sent < update.total);

    // A node that is told but never asks.
    rig.config.fw_stall_after.store(0, Ordering::Relaxed);
    rig.config.fw_never_reads.store(true, Ordering::Relaxed);
    rig.service
        .start_update(NODE, image(NEW_CRC, 3000), false)
        .await
        .unwrap();
    wait_until("the start timeout", || {
        rig.service
            .status()
            .update
            .is_some_and(|u| u.phase == UpdatePhase::Failed)
    })
    .await;
    assert!(rig
        .service
        .status()
        .update
        .unwrap()
        .message
        .contains("never asked"));
}

#[tokio::test]
async fn an_update_can_be_cancelled_and_only_one_runs_at_a_time() {
    let rig = started(1).await;
    assert!(
        rig.service.cancel_update().is_err(),
        "nothing to cancel yet"
    );
    rig.config.fw_never_reads.store(true, Ordering::Relaxed);
    rig.service
        .start_update(NODE, image(NEW_CRC, 3000), false)
        .await
        .unwrap();
    let second = rig
        .service
        .start_update(NODE, image(NEW_CRC, 3000), false)
        .await
        .unwrap_err();
    assert!(
        second.to_string().contains("another") || second.to_string().contains("busy"),
        "{second}"
    );
    rig.service.cancel_update().unwrap();
    let update = rig.service.status().update.unwrap();
    assert_eq!(
        (update.phase, update.message.as_str()),
        (UpdatePhase::Failed, "cancelled")
    );
}

#[tokio::test]
async fn image_files_are_described_before_they_are_sent() {
    let info = DroneCanService::inspect_image(&image(NEW_CRC, 500)).unwrap();
    assert_eq!((info.format, info.size), ("bin", 500));
    assert_eq!(info.crc.as_deref(), Some("AAAABBBBCCCCDDDD"));
    let plain = DroneCanService::inspect_image(&[7; 300]).unwrap();
    assert_eq!(
        plain.crc, None,
        "a file without a descriptor still works but is flagged"
    );
    assert!(DroneCanService::inspect_image(&[]).is_err());
    assert!(DroneCanService::inspect_image(b"{ not an apj }").is_err());
}

// ---- dynamic node id allocation ----

/// The replies of the ground station allocator that went out on the bus.
fn allocation_replies(vehicle: &Vehicle) -> Vec<Value> {
    let ty = index("uavcan.protocol.dynamic_node_id.Allocation");
    let signature_of = |kind: &TransferKind| match kind {
        TransferKind::Message { type_id } => catalog().message(*type_id).map(|t| t.signature),
        _ => None,
    };
    // The answers are longer than one frame once the unique id grows, so reassemble them.
    let mut reassembler = Reassembler::new();
    let now = Instant::now();
    let frames: Vec<CanFrame> = vehicle
        .received
        .lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MavMessage::CAN_FRAME(f) => Some(CanFrame {
                id: f.id & 0x1FFF_FFFF,
                data: f.data[..usize::from(f.len)].to_vec(),
            }),
            _ => None,
        })
        .collect();
    frames
        .iter()
        .filter_map(|frame| reassembler.push(frame, now, signature_of))
        .filter(|t| t.source == 127 && t.kind == TransferKind::Message { type_id: 1 })
        .map(|t| {
            catalog()
                .decode(ty, Side::Message, &t.payload, true)
                .unwrap()
        })
        .collect()
}

fn allocation_transfer(request: &Value) -> Vec<CanFrame> {
    let ty = index("uavcan.protocol.dynamic_node_id.Allocation");
    let payload = catalog().encode(ty, Side::Message, request, true).unwrap();
    encode_transfer(
        &Transfer {
            priority: 30,
            source: 0,
            kind: TransferKind::Message { type_id: 1 },
            transfer_id: 0,
            payload,
        },
        catalog().types[ty].signature,
        false,
        0x1234,
    )
}

/// A device without a node id asks for one: one stage of the request, then waits for the answer.
async fn ask_for_id(rig: &Rig, first: bool, preferred: u8, chunk: &[u8]) -> Value {
    let before = allocation_replies(&rig.vehicle).len();
    let request = Value::strukt([
        ("node_id", Value::Uint(u64::from(preferred))),
        ("first_part_of_unique_id", Value::Bool(first)),
        (
            "unique_id",
            Value::Array(chunk.iter().map(|b| Value::Uint(u64::from(*b))).collect()),
        ),
    ]);
    for frame in allocation_transfer(&request) {
        rig.vehicle.send_raw(&frame, 0).await;
    }
    wait_until("the allocator answer", || {
        allocation_replies(&rig.vehicle).len() > before
    })
    .await;
    allocation_replies(&rig.vehicle).remove(before)
}

fn uid(seed: u8) -> Vec<u8> {
    (0..16).map(|i| seed.wrapping_add(i)).collect()
}

async fn allocate(rig: &Rig, preferred: u8, id: &[u8]) -> Value {
    let one = ask_for_id(rig, true, preferred, &id[0..6]).await;
    assert_eq!(one.get("node_id").and_then(Value::as_u64), Some(0));
    assert_eq!(
        one.get("unique_id")
            .and_then(Value::as_array)
            .unwrap()
            .len(),
        6
    );
    let two = ask_for_id(rig, false, preferred, &id[6..12]).await;
    assert_eq!(
        two.get("unique_id")
            .and_then(Value::as_array)
            .unwrap()
            .len(),
        12
    );
    ask_for_id(rig, false, preferred, &id[12..16]).await
}

#[tokio::test]
async fn the_allocator_hands_out_node_ids_in_three_stages() {
    let rig = started(1).await;
    assert!(!rig.service.status().allocation.enabled, "off by default");
    // Off: nobody answers.
    let request = Value::strukt([
        ("first_part_of_unique_id", Value::Bool(true)),
        ("unique_id", Value::Array(vec![Value::Uint(1); 6])),
    ]);
    for frame in allocation_transfer(&request) {
        rig.vehicle.send_raw(&frame, 0).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(allocation_replies(&rig.vehicle).is_empty());

    rig.service.set_allocation(true).unwrap();
    let first = allocate(&rig, 0, &uid(10)).await;
    let first_id = first.get("node_id").and_then(Value::as_u64).unwrap();
    assert_eq!(
        first_id, 125,
        "the highest free id when no preference is given"
    );
    assert_eq!(
        first
            .get("unique_id")
            .and_then(Value::as_array)
            .unwrap()
            .len(),
        16
    );

    // A device that wants a particular id gets it if it is free, otherwise the next free one.
    let wanted = allocate(&rig, 42, &uid(50)).await;
    assert_eq!(wanted.get("node_id").and_then(Value::as_u64), Some(42));
    let taken = allocate(&rig, 42, &uid(90)).await;
    assert_eq!(taken.get("node_id").and_then(Value::as_u64), Some(124));
    // The same device asking again gets the same id back.
    let again = allocate(&rig, 0, &uid(10)).await;
    assert_eq!(again.get("node_id").and_then(Value::as_u64), Some(first_id));

    let state = rig.service.status().allocation;
    assert!(state.enabled);
    assert_eq!(state.recent[0].node_id, 125, "newest first");
    assert_eq!(state.recent.len(), 4);
    assert_eq!(state.recent[3].unique_id.len(), 32);

    rig.service.set_allocation(false).unwrap();
    assert!(!rig.service.status().allocation.enabled);
}

#[tokio::test]
async fn a_known_node_keeps_its_id_and_ids_in_use_are_never_reused() {
    let rig = started(1).await;
    rig.service.set_allocation(true).unwrap();
    // The fake node (id 10) reports the unique id 01..10 in its node info.
    let known: Vec<u8> = (1..=16).collect();
    let reply = allocate(&rig, 0, &known).await;
    assert_eq!(
        reply.get("node_id").and_then(Value::as_u64),
        Some(u64::from(NODE))
    );
    // A device that asks for the id of a node on the bus does not get it.
    let other = allocate(&rig, NODE, &uid(200)).await;
    assert_ne!(
        other.get("node_id").and_then(Value::as_u64),
        Some(u64::from(NODE))
    );
    // Stopping turns the allocator off.
    rig.service.stop().unwrap();
    assert!(
        rig.service.set_allocation(true).is_err(),
        "needs a running relay"
    );
}

// ---- the DroneCAN inspector ----

/// Collects inspector snapshots until `enough` says the messages are all there.
async fn snapshots_until(
    receiver: &mut tokio::sync::broadcast::Receiver<control_station_link::InspectorSnapshot>,
    mut enough: impl FnMut(&control_station_link::InspectorSnapshot) -> bool,
) -> Vec<control_station_link::InspectorSnapshot> {
    let mut all = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(snapshot)) =
            tokio::time::timeout(Duration::from_millis(500), receiver.recv()).await
        {
            let done = enough(&snapshot);
            all.push(snapshot);
            if done {
                return all;
            }
        }
    }
    panic!("the inspector never showed what was expected");
}

#[tokio::test]
async fn the_inspector_counts_traffic_per_node_and_kind_and_decodes_it() {
    let rig = started(1).await;
    let mut receiver = rig.service.subscribe_inspector();
    rig.service.inspector_enable(true);
    rig.service.inspector_watch("10/0/341/uptime_sec").unwrap();
    assert!(rig.service.inspector_watch("nonsense").is_err());
    // Requests and responses on the bus: the node answers a parameter listing and a write. The
    // write comes last so the latest GetSet response carries a value.
    rig.service.list_params(NODE).await.unwrap();
    rig.service
        .set_param(NODE, "GPS_TYPE", NodeParamValue::Integer(9))
        .await
        .unwrap();

    let snapshots = snapshots_until(&mut receiver, |s| {
        let has = |node: u8, kind: u8, id: u32| {
            s.messages
                .iter()
                .any(|m| (m.system_id, m.component_id, m.message_id) == (node, kind, id))
        };
        has(10, 0, 341) && has(10, 2, 11) && has(10, 0, 16383)
    })
    .await;
    let last = snapshots.last().unwrap();
    let find = |node: u8, kind: u8, id: u32| {
        last.messages
            .iter()
            .find(|m| (m.system_id, m.component_id, m.message_id) == (node, kind, id))
            .unwrap_or_else(|| panic!("no message {node}/{kind}/{id}"))
    };
    let status = find(10, 0, 341);
    assert_eq!(
        (status.name, status.component_name.as_str()),
        ("NodeStatus", "Broadcast")
    );
    assert!(status.hz > 0.0 && status.count >= 1 && status.bytes_per_second > 0.0);
    let field = |m: &control_station_link::InspectorMessage, name: &str| {
        m.fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value.clone())
    };
    assert_eq!(field(status, "uptime_sec"), Some(serde_json::json!(42)));
    assert_eq!(field(status, "mode"), Some(serde_json::json!(0)));

    let log = find(10, 0, 16383);
    assert_eq!(field(log, "text"), Some(serde_json::json!("no fix yet")));
    assert_eq!(field(log, "level.value"), Some(serde_json::json!(2)));

    let response = find(10, 2, 11);
    assert_eq!(
        (response.name, response.component_name.as_str()),
        ("GetSet", "Service responses")
    );
    assert_eq!(field(response, "name"), Some(serde_json::json!("GPS_TYPE")));
    assert_eq!(
        field(response, "value.integer_value"),
        Some(serde_json::json!(9)),
        "unions show under their variant: {:?}",
        response.fields
    );

    // The watched series was recorded and handed out once.
    let points: Vec<_> = snapshots
        .iter()
        .filter_map(|s| s.series.get("10/0/341/uptime_sec"))
        .flatten()
        .collect();
    assert!(!points.is_empty());
    assert!(points.iter().all(|p| p.1 == 42.0));
}

#[tokio::test]
async fn a_disabled_inspector_stays_silent_and_clear_resets_the_counters() {
    let rig = started(1).await;
    let mut receiver = rig.service.subscribe_inspector();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        receiver.try_recv().is_err(),
        "nothing is emitted while it is off"
    );

    rig.service.inspector_enable(true);
    let first = snapshots_until(&mut receiver, |s| s.messages.iter().any(|m| m.count >= 20)).await;
    let before = first
        .last()
        .unwrap()
        .messages
        .iter()
        .map(|m| m.count)
        .max()
        .unwrap();
    rig.service.inspector_clear();
    let after = snapshots_until(&mut receiver, |s| !s.messages.is_empty()).await;
    let restarted = after
        .last()
        .unwrap()
        .messages
        .iter()
        .map(|m| m.count)
        .max()
        .unwrap();
    assert!(
        restarted < before,
        "the counters started again ({restarted} after, {before} before)"
    );

    // Stopping DroneCAN turns the inspector off.
    rig.service.stop().unwrap();
    while receiver.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(receiver.try_recv().is_err());
}
