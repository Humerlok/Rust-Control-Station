//! DroneCAN over the MAVLink CAN forward: the autopilot relays one of its CAN buses as
//! `CAN_FRAME` / `CANFD_FRAME` messages, and this service speaks DroneCAN on top of that as node
//! 127. It keeps the node table (status and node info), collects node log messages and runs the
//! request/response services the screen needs: parameters, save and restart.
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use control_station_commands as commands;
use control_station_dronecan::{
    transfer::{encode_transfer, CanFrame, Reassembler, Transfer, TransferKind, GCS_PRIORITY},
    Catalog, Side, Value,
};
use control_station_mavlink::{
    dialect::{MavCmd, CANFD_FRAME_DATA, CAN_FRAME_DATA},
    MavMessage, Packet,
};
use control_station_vehicle_state::VehicleId;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use crate::{
    manager::LinkManager,
    types::{LinkError, LinkEvent, LinkState},
};

/// The node id the ground station uses on the bus.
pub const GCS_NODE_ID: u8 = 127;
const EXTENDED_FLAG: u32 = 0x8000_0000;
const MAX_LOGS: usize = 100;
const MAX_PARAMS: u32 = 2000;
const RESTART_MAGIC: u64 = 0xACCE_551B1E;
const GCS_NODE_NAME: &str = "org.ardupilot.controlstation";

#[derive(Debug, Clone, Copy)]
pub struct DroneCanTiming {
    /// How often CAN forward is renewed (the autopilot lets it lapse after about 5 s).
    pub forward_interval: Duration,
    /// How often the ground station announces itself with a NodeStatus.
    pub status_interval: Duration,
    /// How often a node whose info is missing is asked for it.
    pub info_interval: Duration,
    pub request_timeout: Duration,
    pub request_attempts: u32,
    /// A node that has not sent NodeStatus for this long is shown offline.
    pub offline_after: Duration,
    pub tick: Duration,
}

impl Default for DroneCanTiming {
    fn default() -> Self {
        Self {
            forward_interval: Duration::from_secs(1),
            status_interval: Duration::from_secs(1),
            info_interval: Duration::from_secs(2),
            request_timeout: Duration::from_secs(1),
            request_attempts: 4,
            offline_after: Duration::from_secs(3),
            tick: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroneCanNode {
    pub id: u8,
    pub online: bool,
    pub name: Option<String>,
    pub health: String,
    pub mode: String,
    pub uptime_sec: u32,
    pub vendor_status: u16,
    pub sw_version: Option<String>,
    pub sw_crc: Option<String>,
    pub hw_version: Option<String>,
    pub unique_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroneCanLog {
    pub time_ms: u64,
    pub node: u8,
    pub level: String,
    pub source: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamsProgress {
    pub node: u8,
    pub received: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroneCanState {
    pub active: bool,
    /// 1 or 2: the autopilot's CAN bus being relayed.
    pub bus: Option<u8>,
    pub nodes: Vec<DroneCanNode>,
    pub logs: Vec<DroneCanLog>,
    pub frames_rx: u64,
    pub frames_tx: u64,
    pub progress: Option<ParamsProgress>,
}

/// A node parameter value; the screen picks the variant from the parameter's current one.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum NodeParamValue {
    Empty,
    Integer(i64),
    Real(f64),
    Boolean(bool),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeParam {
    pub index: u32,
    pub name: String,
    pub value: NodeParamValue,
    pub default: Option<NodeParamValue>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl NodeParamValue {
    fn to_value(&self) -> Value {
        match self {
            Self::Empty => Value::union("empty", Value::Struct(vec![])),
            Self::Integer(v) => Value::union("integer_value", Value::Int(*v)),
            Self::Real(v) => Value::union("real_value", Value::Float(*v)),
            Self::Boolean(v) => Value::union("boolean_value", Value::Uint(u64::from(*v))),
            Self::Text(v) => Value::union("string_value", Value::text(v)),
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        let (variant, inner) = value.variant()?;
        Some(match variant {
            "empty" => Self::Empty,
            "integer_value" => Self::Integer(inner.as_i64()?),
            "real_value" => Self::Real(inner.as_f64()?),
            "boolean_value" => Self::Boolean(inner.as_bool()?),
            "string_value" => Self::Text(inner.as_text()?),
            _ => return None,
        })
    }

    fn numeric(&self) -> Option<f64> {
        match self {
            Self::Integer(v) => Some(*v as f64),
            Self::Real(v) => Some(*v),
            Self::Boolean(v) => Some(f64::from(u8::from(*v))),
            _ => None,
        }
    }
}

struct Types {
    node_status: usize,
    get_node_info: usize,
    restart: usize,
    get_set: usize,
    execute_opcode: usize,
    log_message: usize,
}

struct NodeEntry {
    node: DroneCanNode,
    last_seen: Instant,
    last_info_request: Option<Instant>,
}

type Reply = (u8, Value);

struct Inner {
    bus: Option<u8>,
    target: Option<VehicleId>,
    fd: bool,
    nodes: BTreeMap<u8, NodeEntry>,
    logs: VecDeque<DroneCanLog>,
    reassembler: Reassembler,
    /// Waiting requests by (node, service id, transfer id).
    pending: HashMap<(u8, u8, u8), mpsc::UnboundedSender<Reply>>,
    transfer_ids: HashMap<(u8, u8), u8>,
    broadcast_id: u8,
    progress: Option<ParamsProgress>,
    busy_nodes: Vec<u8>,
    frames_rx: u64,
    frames_tx: u64,
    dirty: bool,
    last_emit: Instant,
    started: Instant,
}

type Receivers = (
    broadcast::Receiver<Arc<Packet>>,
    broadcast::Receiver<LinkEvent>,
);

struct Shared {
    link: LinkManager,
    catalog: &'static Catalog,
    types: Types,
    timing: DroneCanTiming,
    inner: Mutex<Inner>,
    events: broadcast::Sender<DroneCanState>,
    receivers: Mutex<Option<Receivers>>,
}

#[derive(Clone)]
pub struct DroneCanService {
    shared: Arc<Shared>,
}

fn error(text: impl Into<String>) -> LinkError {
    LinkError::Other(text.into())
}

fn health_name(code: u64) -> &'static str {
    match code {
        0 => "OK",
        1 => "WARNING",
        2 => "ERROR",
        _ => "CRITICAL",
    }
}

fn mode_name(code: u64) -> &'static str {
    match code {
        0 => "OPERATIONAL",
        1 => "INITIALIZATION",
        2 => "MAINTENANCE",
        3 => "SOFTWARE_UPDATE",
        7 => "OFFLINE",
        _ => "UNKNOWN",
    }
}

fn level_name(code: u64) -> &'static str {
    match code {
        0 => "DEBUG",
        1 => "INFO",
        2 => "WARNING",
        _ => "ERROR",
    }
}

fn hex(bytes: impl IntoIterator<Item = u8>) -> String {
    bytes.into_iter().map(|b| format!("{b:02X}")).collect()
}

impl DroneCanService {
    pub fn new(link: LinkManager) -> Self {
        Self::with_timing(link, DroneCanTiming::default())
    }

    pub fn with_timing(link: LinkManager, timing: DroneCanTiming) -> Self {
        let catalog = Catalog::builtin();
        let index = |name: &str| {
            catalog
                .index_of(name)
                .unwrap_or_else(|| panic!("DSDL type {name}"))
        };
        let types = Types {
            node_status: index("uavcan.protocol.NodeStatus"),
            get_node_info: index("uavcan.protocol.GetNodeInfo"),
            restart: index("uavcan.protocol.RestartNode"),
            get_set: index("uavcan.protocol.param.GetSet"),
            execute_opcode: index("uavcan.protocol.param.ExecuteOpcode"),
            log_message: index("uavcan.protocol.debug.LogMessage"),
        };
        let (events, _) = broadcast::channel(64);
        let receivers = (link.subscribe_packets(), link.subscribe());
        let now = Instant::now();
        Self {
            shared: Arc::new(Shared {
                link,
                catalog,
                types,
                timing,
                inner: Mutex::new(Inner {
                    bus: None,
                    target: None,
                    fd: false,
                    nodes: BTreeMap::new(),
                    logs: VecDeque::new(),
                    reassembler: Reassembler::new(),
                    pending: HashMap::new(),
                    transfer_ids: HashMap::new(),
                    broadcast_id: 0,
                    progress: None,
                    busy_nodes: Vec::new(),
                    frames_rx: 0,
                    frames_tx: 0,
                    dirty: false,
                    last_emit: now,
                    started: now,
                }),
                events,
                receivers: Mutex::new(Some(receivers)),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DroneCanState> {
        self.shared.events.subscribe()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.shared.inner.lock().expect("dronecan state lock")
    }

    fn snapshot(inner: &Inner) -> DroneCanState {
        DroneCanState {
            active: inner.bus.is_some(),
            bus: inner.bus,
            nodes: inner.nodes.values().map(|e| e.node.clone()).collect(),
            logs: inner.logs.iter().cloned().collect(),
            frames_rx: inner.frames_rx,
            frames_tx: inner.frames_tx,
            progress: inner.progress.clone(),
        }
    }

    pub fn status(&self) -> DroneCanState {
        Self::snapshot(&self.lock())
    }

    fn emit(&self, inner: &mut Inner) {
        inner.dirty = false;
        inner.last_emit = Instant::now();
        let _ = self.shared.events.send(Self::snapshot(inner));
    }

    /// Starts relaying a CAN bus of the autopilot (1 or 2) and speaking DroneCAN on it.
    pub fn start(&self, bus: u8) -> Result<String, LinkError> {
        if !(1..=2).contains(&bus) {
            return Err(error("the CAN bus must be 1 or 2"));
        }
        let (target, _) = self.shared.link.selected_vehicle()?;
        {
            let mut inner = self.lock();
            if inner.bus.is_some() {
                return Err(error("DroneCAN is already running; stop it first"));
            }
            inner.bus = Some(bus);
            inner.target = Some(target);
            inner.nodes.clear();
            inner.logs.clear();
            inner.reassembler = Reassembler::new();
            inner.started = Instant::now();
            self.emit(&mut inner);
        }
        self.send_forward(target, bus);
        Ok(format!("Relaying CAN{bus}"))
    }

    /// Stops the relay (the autopilot stops forwarding frames when the command is renewed with 0).
    pub fn stop(&self) -> Result<String, LinkError> {
        let target = {
            let mut inner = self.lock();
            if inner.bus.take().is_none() {
                return Ok("DroneCAN is not running".into());
            }
            inner.nodes.clear();
            inner.pending.clear();
            inner.progress = None;
            inner.busy_nodes.clear();
            self.emit(&mut inner);
            inner.target
        };
        if let Some(target) = target {
            let _ = self.shared.link.send(&commands::command_long(
                target,
                MavCmd::MAV_CMD_CAN_FORWARD,
                [0.0; 7],
            ));
        }
        Ok("Stopped".into())
    }

    fn send_forward(&self, target: VehicleId, bus: u8) {
        let _ = self.shared.link.send(&commands::command_long(
            target,
            MavCmd::MAV_CMD_CAN_FORWARD,
            [f32::from(bus), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ));
    }

    /// Consumes the packet bus until the link manager goes away. Spawn it once.
    pub async fn run(&self) {
        let Some((mut packets, mut events)) = self
            .shared
            .receivers
            .lock()
            .expect("dronecan receivers lock")
            .take()
        else {
            return;
        };
        let timing = self.shared.timing;
        let mut tick = tokio::time::interval(timing.tick);
        let mut last_forward = Instant::now();
        let mut last_status = Instant::now();
        loop {
            tokio::select! {
                packet = packets.recv() => match packet {
                    Ok(packet) => self.handle_packet(&packet).await,
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                event = events.recv() => match event {
                    Ok(LinkEvent::Link(info)) if info.state != LinkState::Connected => {
                        let _ = self.stop();
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                _ = tick.tick() => {
                    let now = Instant::now();
                    let (bus, target) = {
                        let inner = self.lock();
                        (inner.bus, inner.target)
                    };
                    if let (Some(bus), Some(target)) = (bus, target) {
                        if now.duration_since(last_forward) >= timing.forward_interval {
                            last_forward = now;
                            self.send_forward(target, bus);
                        }
                        if now.duration_since(last_status) >= timing.status_interval {
                            last_status = now;
                            self.announce().await;
                        }
                        self.ask_for_node_info().await;
                    }
                    self.housekeeping(now);
                }
            }
        }
    }

    /// Marks silent nodes offline and emits a state that changed.
    fn housekeeping(&self, now: Instant) {
        let mut inner = self.lock();
        let offline_after = self.shared.timing.offline_after;
        let mut changed = false;
        for entry in inner.nodes.values_mut() {
            let online = now.duration_since(entry.last_seen) < offline_after;
            if entry.node.online != online {
                entry.node.online = online;
                changed = true;
            }
        }
        inner.dirty |= changed;
        if inner.dirty && now.duration_since(inner.last_emit) >= Duration::from_millis(250) {
            self.emit(&mut inner);
        }
    }

    fn signature_of(&self, kind: &TransferKind) -> Option<u64> {
        let catalog = self.shared.catalog;
        match kind {
            TransferKind::Message { type_id } => catalog.message(*type_id).map(|t| t.signature),
            TransferKind::Request { service_id, .. }
            | TransferKind::Response { service_id, .. } => {
                catalog.service(u16::from(*service_id)).map(|t| t.signature)
            }
        }
    }

    fn type_index(&self, kind: &TransferKind) -> Option<usize> {
        let catalog = self.shared.catalog;
        let full_name = match kind {
            TransferKind::Message { type_id } => &catalog.message(*type_id)?.full_name,
            TransferKind::Request { service_id, .. }
            | TransferKind::Response { service_id, .. } => {
                &catalog.service(u16::from(*service_id))?.full_name
            }
        };
        catalog.index_of(full_name)
    }

    async fn handle_packet(&self, packet: &Packet) {
        let (frame, fd) = match &packet.message {
            MavMessage::CAN_FRAME(m) => (
                self.frame_of(packet, m.bus, m.id, &m.data[..usize::from(m.len).min(8)]),
                false,
            ),
            MavMessage::CANFD_FRAME(m) => (
                self.frame_of(packet, m.bus, m.id, &m.data[..usize::from(m.len).min(64)]),
                true,
            ),
            _ => return,
        };
        let Some(frame) = frame else { return };
        let transfer = {
            let mut inner = self.lock();
            inner.frames_rx += 1;
            inner.fd = fd;
            let signature_of = |kind: &TransferKind| self.signature_of(kind);
            inner.reassembler.push(&frame, Instant::now(), signature_of)
        };
        if let Some(transfer) = transfer {
            self.handle_transfer(transfer, fd).await;
        }
    }

    /// A relayed frame of the bus and vehicle in use, as a CAN frame.
    fn frame_of(&self, packet: &Packet, bus: u8, id: u32, data: &[u8]) -> Option<CanFrame> {
        let inner = self.lock();
        let (active, target) = (inner.bus?, inner.target?);
        if bus != active - 1 || packet.system_id() != target.system_id || id & EXTENDED_FLAG == 0 {
            return None;
        }
        Some(CanFrame {
            id: id & 0x1FFF_FFFF,
            data: data.to_vec(),
        })
    }

    async fn handle_transfer(&self, transfer: Transfer, fd: bool) {
        let catalog = self.shared.catalog;
        let types = &self.shared.types;
        let Some(index) = self.type_index(&transfer.kind) else {
            return;
        };
        match transfer.kind {
            TransferKind::Message { .. } if index == types.node_status => {
                if let Ok(value) = catalog.decode(index, Side::Message, &transfer.payload, !fd) {
                    self.update_status(transfer.source, &value);
                }
            }
            TransferKind::Message { .. } if index == types.log_message => {
                if let Ok(value) = catalog.decode(index, Side::Message, &transfer.payload, !fd) {
                    self.add_log(transfer.source, &value);
                }
            }
            TransferKind::Response {
                service_id,
                destination,
            } if destination == GCS_NODE_ID => {
                if let Ok(value) = catalog.decode(index, Side::Response, &transfer.payload, !fd) {
                    let key = (transfer.source, service_id, transfer.transfer_id);
                    let sender = self.lock().pending.get(&key).cloned();
                    if let Some(sender) = sender {
                        let _ = sender.send((transfer.transfer_id, value));
                    }
                }
            }
            TransferKind::Request { destination, .. }
                if destination == GCS_NODE_ID && index == types.get_node_info =>
            {
                self.answer_node_info(transfer.source, transfer.transfer_id)
                    .await;
            }
            _ => {}
        }
    }

    fn update_status(&self, node: u8, status: &Value) {
        if node == 0 || node == GCS_NODE_ID {
            return;
        }
        let health = status.get("health").and_then(Value::as_u64).unwrap_or(0);
        let mode = status.get("mode").and_then(Value::as_u64).unwrap_or(0);
        let mut inner = self.lock();
        let entry = inner.nodes.entry(node).or_insert_with(|| NodeEntry {
            node: DroneCanNode {
                id: node,
                online: true,
                name: None,
                health: String::new(),
                mode: String::new(),
                uptime_sec: 0,
                vendor_status: 0,
                sw_version: None,
                sw_crc: None,
                hw_version: None,
                unique_id: None,
            },
            last_seen: Instant::now(),
            last_info_request: None,
        });
        entry.last_seen = Instant::now();
        entry.node.online = true;
        entry.node.health = health_name(health).into();
        entry.node.mode = mode_name(mode).into();
        entry.node.uptime_sec = status
            .get("uptime_sec")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        entry.node.vendor_status = status
            .get("vendor_specific_status_code")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u16;
        inner.dirty = true;
    }

    fn update_info(&self, node: u8, info: &Value) {
        let mut inner = self.lock();
        let Some(entry) = inner.nodes.get_mut(&node) else {
            return;
        };
        let get_u64 = |path: &str| info.get(path).and_then(Value::as_u64).unwrap_or(0);
        entry.node.name = info.get("name").and_then(Value::as_text);
        let flags = get_u64("software_version.optional_field_flags");
        let mut version = format!(
            "{}.{}",
            get_u64("software_version.major"),
            get_u64("software_version.minor")
        );
        if flags & 1 != 0 {
            version.push_str(&format!(
                " ({:08x})",
                get_u64("software_version.vcs_commit")
            ));
        }
        entry.node.sw_version = Some(version);
        entry.node.sw_crc =
            (flags & 2 != 0).then(|| format!("{:016X}", get_u64("software_version.image_crc")));
        entry.node.hw_version = Some(format!(
            "{}.{}",
            get_u64("hardware_version.major"),
            get_u64("hardware_version.minor")
        ));
        entry.node.unique_id = info
            .get("hardware_version.unique_id")
            .and_then(Value::as_array)
            .map(|ids| hex(ids.iter().filter_map(|v| v.as_u64()).map(|b| b as u8)));
        inner.dirty = true;
    }

    fn add_log(&self, node: u8, log: &Value) {
        let mut inner = self.lock();
        let time_ms = inner.started.elapsed().as_millis() as u64;
        inner.logs.push_front(DroneCanLog {
            time_ms,
            node,
            level: level_name(log.get("level.value").and_then(Value::as_u64).unwrap_or(1)).into(),
            source: log
                .get("source")
                .and_then(Value::as_text)
                .unwrap_or_default(),
            text: log.get("text").and_then(Value::as_text).unwrap_or_default(),
        });
        inner.logs.truncate(MAX_LOGS);
        inner.dirty = true;
    }

    // ---- sending ----

    fn next_transfer_id(&self, node: u8, service: u8) -> u8 {
        let mut inner = self.lock();
        let id = inner.transfer_ids.entry((node, service)).or_insert(0);
        let current = *id;
        *id = (*id + 1) & 0x1F;
        current
    }

    async fn send_frames(&self, frames: Vec<CanFrame>) -> Result<(), LinkError> {
        let (bus, target, fd) = {
            let inner = self.lock();
            (
                inner.bus.ok_or_else(|| error("DroneCAN is not running"))?,
                inner.target.ok_or(LinkError::NoVehicle)?,
                inner.fd,
            )
        };
        for frame in frames {
            let message = if frame.data.len() > 8 || fd {
                let mut data = [0u8; 64];
                data[..frame.data.len()].copy_from_slice(&frame.data);
                MavMessage::CANFD_FRAME(CANFD_FRAME_DATA {
                    id: frame.id | EXTENDED_FLAG,
                    target_system: target.system_id,
                    target_component: target.component_id,
                    bus: bus - 1,
                    len: frame.data.len() as u8,
                    data,
                })
            } else {
                let mut data = [0u8; 8];
                data[..frame.data.len()].copy_from_slice(&frame.data);
                MavMessage::CAN_FRAME(CAN_FRAME_DATA {
                    id: frame.id | EXTENDED_FLAG,
                    target_system: target.system_id,
                    target_component: target.component_id,
                    bus: bus - 1,
                    len: frame.data.len() as u8,
                    data,
                })
            };
            // The link queue is short; wait for room instead of losing the frame.
            let mut attempts = 0;
            loop {
                match self.shared.link.send(&message) {
                    Ok(()) => break,
                    Err(LinkError::Busy) if attempts < 200 => {
                        attempts += 1;
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    Err(e) => return Err(e),
                }
            }
            self.lock().frames_tx += 1;
        }
        Ok(())
    }

    async fn send_transfer(&self, transfer: Transfer, signature: u64) -> Result<(), LinkError> {
        let fd = self.lock().fd;
        self.send_frames(encode_transfer(&transfer, signature, fd, 0))
            .await
    }

    /// The ground station's own NodeStatus.
    async fn announce(&self) {
        let types = &self.shared.types;
        let catalog = self.shared.catalog;
        let uptime = self.lock().started.elapsed().as_secs();
        let status = Value::strukt([
            ("uptime_sec", Value::Uint(uptime)),
            ("health", Value::Uint(0)),
            ("mode", Value::Uint(0)),
            ("sub_mode", Value::Uint(0)),
            ("vendor_specific_status_code", Value::Uint(0)),
        ]);
        let fd = self.lock().fd;
        let Ok(payload) = catalog.encode(types.node_status, Side::Message, &status, !fd) else {
            return;
        };
        let transfer_id = {
            let mut inner = self.lock();
            let id = inner.broadcast_id;
            inner.broadcast_id = (id + 1) & 0x1F;
            id
        };
        let def = &catalog.types[types.node_status];
        let transfer = Transfer {
            priority: GCS_PRIORITY,
            source: GCS_NODE_ID,
            kind: TransferKind::Message {
                type_id: def.id.unwrap_or(341),
            },
            transfer_id,
            payload,
        };
        let _ = self.send_transfer(transfer, def.signature).await;
    }

    async fn answer_node_info(&self, requester: u8, transfer_id: u8) {
        let types = &self.shared.types;
        let catalog = self.shared.catalog;
        let uptime = self.lock().started.elapsed().as_secs();
        let response = Value::strukt([
            (
                "status",
                Value::strukt([("uptime_sec", Value::Uint(uptime))]),
            ),
            ("name", Value::text(GCS_NODE_NAME)),
        ]);
        let fd = self.lock().fd;
        let Ok(payload) = catalog.encode(types.get_node_info, Side::Response, &response, !fd)
        else {
            return;
        };
        let def = &catalog.types[types.get_node_info];
        let transfer = Transfer {
            priority: GCS_PRIORITY,
            source: GCS_NODE_ID,
            kind: TransferKind::Response {
                service_id: def.id.unwrap_or(1) as u8,
                destination: requester,
            },
            transfer_id,
            payload,
        };
        let _ = self.send_transfer(transfer, def.signature).await;
    }

    /// Asks every node that has not told us about itself yet.
    async fn ask_for_node_info(&self) {
        let wanted: Vec<u8> = {
            let mut inner = self.lock();
            let interval = self.shared.timing.info_interval;
            let now = Instant::now();
            inner
                .nodes
                .values_mut()
                .filter(|e| e.node.online && e.node.name.is_none())
                .filter(|e| {
                    e.last_info_request
                        .is_none_or(|t| now.duration_since(t) >= interval)
                })
                .map(|e| {
                    e.last_info_request = Some(now);
                    e.node.id
                })
                .collect()
        };
        for node in wanted {
            let service = self.clone();
            tokio::spawn(async move {
                let request = Value::Struct(vec![]);
                if let Ok(info) = service
                    .call(node, service.shared.types.get_node_info, request, 1)
                    .await
                {
                    service.update_info(node, &info);
                }
            });
        }
    }

    /// Sends a service request and waits for the response, sending it again on silence.
    async fn call(
        &self,
        node: u8,
        type_index: usize,
        request: Value,
        attempts: u32,
    ) -> Result<Value, LinkError> {
        let catalog = self.shared.catalog;
        let def = &catalog.types[type_index];
        let service_id = def.id.unwrap_or(0) as u8;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut keys = Vec::new();
        let result = async {
            for _ in 0..attempts.max(1) {
                let transfer_id = self.next_transfer_id(node, service_id);
                let key = (node, service_id, transfer_id);
                self.lock().pending.insert(key, tx.clone());
                keys.push(key);
                let fd = self.lock().fd;
                let payload = catalog
                    .encode(type_index, Side::Request, &request, !fd)
                    .map_err(|e| error(e.to_string()))?;
                self.send_transfer(
                    Transfer {
                        priority: GCS_PRIORITY,
                        source: GCS_NODE_ID,
                        kind: TransferKind::Request {
                            service_id,
                            destination: node,
                        },
                        transfer_id,
                        payload,
                    },
                    def.signature,
                )
                .await?;
                if let Ok(Some((_, value))) =
                    tokio::time::timeout(self.shared.timing.request_timeout, rx.recv()).await
                {
                    return Ok(value);
                }
            }
            Err(error(format!("node {node} did not answer")))
        }
        .await;
        let mut inner = self.lock();
        for key in keys {
            inner.pending.remove(&key);
        }
        result
    }

    fn begin_node(&self, node: u8) -> Result<(), LinkError> {
        let mut inner = self.lock();
        if inner.bus.is_none() {
            return Err(error("DroneCAN is not running; connect to a CAN bus first"));
        }
        if inner.busy_nodes.contains(&node) {
            return Err(error(format!("node {node} is busy with another operation")));
        }
        inner.busy_nodes.push(node);
        Ok(())
    }

    fn end_node(&self, node: u8) {
        let mut inner = self.lock();
        inner.busy_nodes.retain(|n| *n != node);
        inner.progress = None;
        inner.dirty = true;
    }

    // ---- node services ----

    fn param_from_response(index: u32, response: &Value) -> Option<NodeParam> {
        let name = response.get("name").and_then(Value::as_text)?;
        let value = NodeParamValue::from_value(response.get("value")?)?;
        let default = response
            .get("default_value")
            .and_then(NodeParamValue::from_value)
            .filter(|v| *v != NodeParamValue::Empty);
        let limit = |path: &str| {
            response
                .get(path)
                .and_then(NodeParamValue::from_value)
                .and_then(|v| v.numeric())
        };
        Some(NodeParam {
            index,
            name,
            value,
            default,
            min: limit("min_value"),
            max: limit("max_value"),
        })
    }

    /// Downloads every parameter of a node (by index, until the node answers with an empty name).
    pub async fn list_params(&self, node: u8) -> Result<Vec<NodeParam>, LinkError> {
        self.begin_node(node)?;
        let result = self.list_params_inner(node).await;
        self.end_node(node);
        result
    }

    async fn list_params_inner(&self, node: u8) -> Result<Vec<NodeParam>, LinkError> {
        let mut params = Vec::new();
        for index in 0..MAX_PARAMS {
            let request = Value::strukt([
                ("index", Value::Uint(u64::from(index))),
                ("value", NodeParamValue::Empty.to_value()),
                ("name", Value::text("")),
            ]);
            let response = self
                .call(
                    node,
                    self.shared.types.get_set,
                    request,
                    self.shared.timing.request_attempts,
                )
                .await
                .map_err(|e| error(format!("{e} (after {index} parameters)")))?;
            let Some(param) = Self::param_from_response(index, &response) else {
                break;
            };
            if param.name.is_empty() {
                break;
            }
            params.push(param);
            let mut inner = self.lock();
            inner.progress = Some(ParamsProgress {
                node,
                received: params.len() as u32,
            });
            inner.dirty = true;
        }
        Ok(params)
    }

    /// Writes one parameter; the node answers with the value it actually holds afterwards.
    pub async fn set_param(
        &self,
        node: u8,
        name: &str,
        value: NodeParamValue,
    ) -> Result<NodeParam, LinkError> {
        if name.is_empty() || name.len() > 92 {
            return Err(error("the parameter name must be 1 to 92 characters"));
        }
        self.begin_node(node)?;
        let request = Value::strukt([
            ("index", Value::Uint(0)),
            ("value", value.to_value()),
            ("name", Value::text(name)),
        ]);
        let result = self
            .call(
                node,
                self.shared.types.get_set,
                request,
                self.shared.timing.request_attempts,
            )
            .await;
        self.end_node(node);
        let response = result?;
        let param = Self::param_from_response(0, &response)
            .filter(|p| p.name == name && p.value != NodeParamValue::Empty)
            .ok_or_else(|| error(format!("node {node} has no parameter {name}")))?;
        Ok(param)
    }

    /// Makes the node write its parameters to flash.
    pub async fn save_params(&self, node: u8) -> Result<String, LinkError> {
        self.begin_node(node)?;
        let request = Value::strukt([("opcode", Value::Uint(0)), ("argument", Value::Int(0))]);
        let result = self
            .call(
                node,
                self.shared.types.execute_opcode,
                request,
                self.shared.timing.request_attempts,
            )
            .await;
        self.end_node(node);
        let response = result?;
        if response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            Ok(format!("Node {node} saved its parameters"))
        } else {
            Err(error(format!("node {node} refused to save its parameters")))
        }
    }

    /// Restarts a node. A node often reboots before it answers, so silence is not an error.
    pub async fn restart_node(&self, node: u8) -> Result<String, LinkError> {
        self.begin_node(node)?;
        let request = Value::strukt([("magic_number", Value::Uint(RESTART_MAGIC))]);
        let result = self.call(node, self.shared.types.restart, request, 2).await;
        self.end_node(node);
        match result {
            Ok(response) if response.get("ok").and_then(Value::as_bool).unwrap_or(false) => {
                Ok(format!("Node {node} is restarting"))
            }
            Ok(_) => Err(error(format!("node {node} refused to restart"))),
            Err(_) => Ok(format!(
                "Restart sent to node {node}; it did not answer (it may already be restarting)"
            )),
        }
    }
}
