use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use control_station_ardupilot_meta::VehicleType;
use control_station_commands as commands;
use control_station_mavlink::{
    encode, gcs_heartbeat, MavHeader, MavMessage, MavlinkVersion, Packet, Parser, GCS_COMPONENT_ID,
    GCS_SYSTEM_ID,
};
use control_station_transport::{Connection, Endpoint, LinkReader, LinkWriter};
use control_station_vehicle_state::{is_vehicle_heartbeat, VehicleId, VehicleState, FIELDS};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

use crate::{
    replay, scan,
    types::{
        ConnectionConfig, LinkError, LinkEvent, LinkInfo, LinkState, LinkStats, TelemetrySnapshot,
        VehicleSummary,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub heartbeat_timeout: Duration,
    pub ack_timeout: Duration,
    pub ack_attempts: u32,
    pub tick: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            heartbeat_timeout: Duration::from_secs(5),
            ack_timeout: Duration::from_secs(1),
            ack_attempts: 3,
            tick: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AckEvent {
    system_id: u8,
    command: u32,
    result: u8,
}

struct Session {
    tx: mpsc::Sender<Vec<u8>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[derive(Default)]
struct Counters {
    bytes_rx: u64,
    bytes_tx: u64,
    packets_rx: u64,
    packets_lost: u64,
    bad_crc: u64,
    last_seq: HashMap<(u8, u8), u8>,
}

#[derive(Default)]
struct Rates {
    last_bytes: u64,
    last_packets: u64,
    /// (received, lost) per second over the last three seconds.
    window: VecDeque<(u64, u64)>,
    last_lost: u64,
    bytes_per_second: f64,
    packets_per_second: f64,
    quality_percent: f64,
}

struct Inner {
    state: LinkState,
    description: String,
    error: Option<String>,
    counters: Counters,
    rates: Rates,
    vehicles: BTreeMap<VehicleId, VehicleState>,
    selected: Option<VehicleId>,
    streams_requested: HashSet<VehicleId>,
    sequence: u8,
    session: Option<Session>,
    dirty: bool,
    local_port: Option<u16>,
    generation: u64,
}

struct Shared {
    inner: Mutex<Inner>,
    events: broadcast::Sender<LinkEvent>,
    acks: broadcast::Sender<AckEvent>,
    /// Every valid packet received, for services (parameters, calibration, inspector).
    packets: broadcast::Sender<Arc<Packet>>,
    timing: Timing,
}

/// Owns at most one active connection and the vehicles seen on it.
#[derive(Clone)]
pub struct LinkManager {
    shared: Arc<Shared>,
}

impl Default for LinkManager {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl Inner {
    fn new() -> Self {
        Self {
            state: LinkState::Disconnected,
            description: String::new(),
            error: None,
            counters: Counters::default(),
            rates: Rates {
                quality_percent: 100.0,
                ..Rates::default()
            },
            vehicles: BTreeMap::new(),
            selected: None,
            streams_requested: HashSet::new(),
            sequence: 0,
            session: None,
            dirty: false,
            local_port: None,
            generation: 0,
        }
    }

    fn reset_session_data(&mut self) {
        self.counters = Counters::default();
        self.rates = Rates {
            quality_percent: 100.0,
            ..Rates::default()
        };
        self.vehicles.clear();
        self.selected = None;
        self.streams_requested.clear();
        self.dirty = true;
    }

    fn info(&self) -> LinkInfo {
        LinkInfo {
            state: self.state,
            description: self.description.clone(),
            error: self.error.clone(),
            stats: LinkStats {
                bytes_rx: self.counters.bytes_rx,
                bytes_tx: self.counters.bytes_tx,
                packets_rx: self.counters.packets_rx,
                packets_lost: self.counters.packets_lost,
                bad_crc: self.counters.bad_crc,
                packets_per_second: self.rates.packets_per_second,
                bytes_per_second: self.rates.bytes_per_second,
                quality_percent: self.rates.quality_percent,
            },
        }
    }

    fn snapshot(&self, timeout: Duration) -> TelemetrySnapshot {
        let now = Instant::now();
        let selected = self.selected.and_then(|id| self.vehicles.get(&id));
        let alive = selected.is_some_and(|v| v.is_alive(now, timeout));
        let vehicle_type = selected.map_or(VehicleType::Unknown, |v| v.vehicle_type);
        TelemetrySnapshot {
            vehicle: selected.map(|v| v.id),
            connected: alive,
            armed: alive && selected.is_some_and(|v| v.armed),
            mode: selected.and_then(VehicleState::mode),
            vehicle_type: vehicle_type.as_str().to_owned(),
            available_modes: vehicle_type
                .modes()
                .iter()
                .map(|(_, name)| (*name).to_owned())
                .collect(),
            fields: selected
                .map(|v| {
                    v.fields()
                        .iter()
                        .map(|(k, value)| ((*k).to_owned(), *value))
                        .collect()
                })
                .unwrap_or_default(),
            position: selected.and_then(|v| v.position),
            home: selected.and_then(|v| v.home),
            vehicles: self
                .vehicles
                .values()
                .map(|v| VehicleSummary {
                    system_id: v.id.system_id,
                    component_id: v.id.component_id,
                    vehicle_type: v.vehicle_type.as_str().to_owned(),
                })
                .collect(),
        }
    }

    /// Encodes with this GCS's identity and queues for the writer task.
    fn queue(&mut self, message: &MavMessage) -> Result<(), LinkError> {
        let Some(session) = &self.session else {
            return Err(LinkError::NoVehicle);
        };
        let header = MavHeader {
            system_id: u32::from(GCS_SYSTEM_ID),
            component_id: GCS_COMPONENT_ID,
            sequence: self.sequence,
        };
        let bytes = encode(header, message, MavlinkVersion::V2);
        session.tx.try_send(bytes).map_err(|_| LinkError::Busy)?;
        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
    }

    fn handle_packet(
        &mut self,
        packet: &Packet,
        events: &broadcast::Sender<LinkEvent>,
        acks: &broadcast::Sender<AckEvent>,
    ) {
        let id = VehicleId {
            system_id: packet.system_id(),
            component_id: packet.component_id(),
        };
        self.counters.packets_rx += 1;
        if let Some(last) = self
            .counters
            .last_seq
            .insert((id.system_id, id.component_id), packet.header.sequence)
        {
            let expected = last.wrapping_add(1);
            if packet.header.sequence != expected {
                self.counters.packets_lost +=
                    u64::from(packet.header.sequence.wrapping_sub(expected));
            }
        }
        let now = Instant::now();
        match &packet.message {
            MavMessage::COMMAND_ACK(ack) => {
                let _ = acks.send(AckEvent {
                    system_id: id.system_id,
                    command: ack.command as u32,
                    result: ack.result as u8,
                });
            }
            MavMessage::HEARTBEAT(h) if is_vehicle_heartbeat(h) => {
                let vehicle = self
                    .vehicles
                    .entry(id)
                    .or_insert_with(|| VehicleState::new(id));
                vehicle.apply(&packet.message, now, unix_ms());
                if self.selected.is_none() {
                    self.selected = Some(id);
                }
                self.dirty = true;
            }
            message => {
                if let Some(vehicle) = self.vehicles.get_mut(&id) {
                    if let Some(text) = vehicle.apply(message, now, unix_ms()) {
                        let _ = events.send(LinkEvent::StatusText(text));
                    }
                    self.dirty = true;
                }
            }
        }
    }

    fn update_rates(&mut self) {
        let packets = self.counters.packets_rx;
        let lost = self.counters.packets_lost;
        let received_delta = packets - self.rates.last_packets;
        let lost_delta = lost - self.rates.last_lost;
        self.rates.bytes_per_second = (self.counters.bytes_rx - self.rates.last_bytes) as f64;
        self.rates.packets_per_second = received_delta as f64;
        self.rates.last_bytes = self.counters.bytes_rx;
        self.rates.last_packets = packets;
        self.rates.last_lost = lost;
        self.rates.window.push_back((received_delta, lost_delta));
        if self.rates.window.len() > 3 {
            self.rates.window.pop_front();
        }
        let (received, lost): (u64, u64) = self
            .rates
            .window
            .iter()
            .fold((0, 0), |(r, l), (dr, dl)| (r + dr, l + dl));
        self.rates.quality_percent = if received + lost == 0 {
            100.0
        } else {
            received as f64 / (received + lost) as f64 * 100.0
        };
    }
}

impl LinkManager {
    pub fn new() -> Self {
        Self::with_timing(Timing::default())
    }

    pub fn with_timing(timing: Timing) -> Self {
        let (events, _) = broadcast::channel(512);
        let (acks, _) = broadcast::channel(64);
        let (packets, _) = broadcast::channel(2048);
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner::new()),
                events,
                acks,
                packets,
                timing,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared.inner.lock().expect("link state lock")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LinkEvent> {
        self.shared.events.subscribe()
    }

    /// Every valid packet received on the link (a slow receiver lags and skips packets).
    pub fn subscribe_packets(&self) -> broadcast::Receiver<Arc<Packet>> {
        self.shared.packets.subscribe()
    }

    /// The selected vehicle and its type, or `NoVehicle` when none is alive.
    pub fn selected_vehicle(&self) -> Result<(VehicleId, VehicleType), LinkError> {
        self.target()
    }

    pub fn link_info(&self) -> LinkInfo {
        self.lock().info()
    }

    pub fn telemetry(&self) -> TelemetrySnapshot {
        self.lock().snapshot(self.shared.timing.heartbeat_timeout)
    }

    /// The local port of a UDP server link (handy when binding port 0).
    pub fn local_port(&self) -> Option<u16> {
        self.lock().local_port
    }

    pub fn select_vehicle(&self, id: VehicleId) {
        let mut inner = self.lock();
        if inner.vehicles.contains_key(&id) {
            inner.selected = Some(id);
            inner.dirty = true;
        }
    }

    pub async fn connect(&self, config: ConnectionConfig) -> Result<(), LinkError> {
        self.disconnect();
        let description = describe(&config);
        {
            let mut inner = self.lock();
            inner.state = LinkState::Connecting;
            inner.description = description;
            inner.error = None;
            inner.reset_session_data();
            let _ = self.shared.events.send(LinkEvent::Link(inner.info()));
        }
        match open_config(&config).await {
            Ok(connection) => {
                self.start_session(connection);
                Ok(())
            }
            Err(error) => {
                let mut inner = self.lock();
                inner.state = LinkState::Disconnected;
                inner.error = Some(error.to_string());
                let _ = self.shared.events.send(LinkEvent::Link(inner.info()));
                Err(error)
            }
        }
    }

    pub fn disconnect(&self) {
        let mut inner = self.lock();
        if inner.session.is_none() && inner.state == LinkState::Disconnected {
            return;
        }
        inner.generation += 1;
        inner.session = None;
        inner.state = LinkState::Disconnected;
        inner.error = None;
        inner.local_port = None;
        inner.reset_session_data();
        let _ = self.shared.events.send(LinkEvent::Link(inner.info()));
        let _ = self.shared.events.send(LinkEvent::Telemetry(
            inner.snapshot(self.shared.timing.heartbeat_timeout),
        ));
    }

    fn start_session(&self, connection: Connection) {
        let Connection {
            reader,
            writer,
            description,
            local_port,
        } = connection;
        let (tx, rx) = mpsc::channel(256);
        let mut inner = self.lock();
        inner.generation += 1;
        let generation = inner.generation;
        let tasks = vec![
            tokio::spawn(reader_task(self.shared.clone(), reader, generation)),
            tokio::spawn(writer_task(self.shared.clone(), writer, rx, generation)),
            tokio::spawn(ticker_task(self.shared.clone(), generation)),
        ];
        inner.session = Some(Session { tx, tasks });
        inner.state = LinkState::Connected;
        inner.description = description;
        inner.local_port = local_port;
        inner.error = None;
        let _ = self.shared.events.send(LinkEvent::Link(inner.info()));
    }

    /// Fire-and-forget send.
    pub fn send(&self, message: &MavMessage) -> Result<(), LinkError> {
        self.lock().queue(message)
    }

    /// Sends a COMMAND_LONG / COMMAND_INT and waits for its COMMAND_ACK, resending on silence.
    pub async fn send_command(&self, message: MavMessage) -> Result<(), LinkError> {
        let timing = self.shared.timing;
        self.send_command_with(message, timing.ack_timeout, timing.ack_attempts)
            .await
    }

    /// Like [`send_command`](Self::send_command) with explicit timing, for commands whose ACK
    /// only comes when a long operation finishes (resending would restart it).
    pub async fn send_command_with(
        &self,
        message: MavMessage,
        ack_timeout: Duration,
        attempts: u32,
    ) -> Result<(), LinkError> {
        let (target_system, command) = match &message {
            MavMessage::COMMAND_LONG(c) => (c.target_system, c.command as u32),
            MavMessage::COMMAND_INT(c) => (c.target_system, c.command as u32),
            _ => return Err(LinkError::Other("not a command message".into())),
        };
        let timing = Timing {
            ack_timeout,
            ack_attempts: attempts,
            ..self.shared.timing
        };
        let mut acks = self.shared.acks.subscribe();
        for _ in 0..timing.ack_attempts {
            self.send(&message)?;
            let mut deadline = tokio::time::Instant::now() + timing.ack_timeout;
            loop {
                match tokio::time::timeout_at(deadline, acks.recv()).await {
                    Err(_) => break,
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                    Ok(Err(broadcast::error::RecvError::Closed)) => return Err(LinkError::NoAck),
                    Ok(Ok(ack)) => {
                        if ack.command != command || ack.system_id != target_system {
                            continue;
                        }
                        match ack.result {
                            0 => return Ok(()),
                            5 => deadline = tokio::time::Instant::now() + timing.ack_timeout,
                            other => return Err(LinkError::Denied(result_name(other))),
                        }
                    }
                }
            }
        }
        Err(LinkError::NoAck)
    }

    fn target(&self) -> Result<(VehicleId, VehicleType), LinkError> {
        let inner = self.lock();
        let alive = inner.session.is_some();
        let vehicle = inner
            .selected
            .and_then(|id| inner.vehicles.get(&id))
            .filter(|v| alive && v.is_alive(Instant::now(), self.shared.timing.heartbeat_timeout))
            .ok_or(LinkError::NoVehicle)?;
        Ok((vehicle.id, vehicle.vehicle_type))
    }

    pub async fn arm(&self, force: bool) -> Result<String, LinkError> {
        let (id, _) = self.target()?;
        self.send_command(commands::arm_disarm(id, true, force))
            .await?;
        Ok("Accepted".into())
    }

    pub async fn disarm(&self, force: bool) -> Result<String, LinkError> {
        let (id, _) = self.target()?;
        self.send_command(commands::arm_disarm(id, false, force))
            .await?;
        Ok("Accepted".into())
    }

    pub async fn set_mode(&self, mode: &str) -> Result<String, LinkError> {
        let (id, vehicle_type) = self.target()?;
        let number = vehicle_type
            .mode_number(mode)
            .ok_or_else(|| LinkError::UnknownMode(mode.to_owned()))?;
        self.send_command(commands::set_mode(id, number)).await?;
        Ok("Accepted".into())
    }

    pub async fn set_waypoint(&self, seq: u16) -> Result<String, LinkError> {
        let (id, _) = self.target()?;
        self.send(&commands::mission_set_current(id, seq))?;
        Ok("Sent".into())
    }

    pub async fn do_action(&self, name: &str) -> Result<String, LinkError> {
        let (id, vehicle_type) = self.target()?;
        let message = commands::action_message(name, id, vehicle_type)
            .ok_or_else(|| LinkError::UnknownAction(name.to_owned()))?;
        self.send_command(message).await?;
        Ok("Accepted".into())
    }

    pub async fn reboot(&self) -> Result<String, LinkError> {
        let (id, _) = self.target()?;
        self.send_command(commands::reboot(id)).await?;
        Ok("Accepted".into())
    }

    /// Reboots the selected vehicle into its bootloader (for firmware flashing).
    pub async fn reboot_to_bootloader(&self) -> Result<String, LinkError> {
        let (id, _) = self.target()?;
        self.send_command(commands::reboot_to_bootloader(id))
            .await?;
        Ok("Accepted".into())
    }

    /// Copters must be in Guided before they accept a takeoff command.
    pub async fn takeoff(&self, altitude_m: f32) -> Result<String, LinkError> {
        let (id, vehicle_type) = self.target()?;
        if vehicle_type == VehicleType::Copter {
            self.set_mode("Guided").await?;
        }
        self.send_command(commands::takeoff(id, altitude_m)).await?;
        Ok("Accepted".into())
    }

    pub async fn fly_to(&self, lat: f64, lon: f64, altitude_m: f32) -> Result<String, LinkError> {
        let (id, _) = self.target()?;
        self.send_command(commands::fly_to(id, lat, lon, altitude_m))
            .await?;
        Ok("Accepted".into())
    }

    pub async fn set_home(&self, lat: f64, lon: f64, altitude_m: f32) -> Result<String, LinkError> {
        let (id, _) = self.target()?;
        self.send_command(commands::set_home(id, lat, lon, altitude_m))
            .await?;
        Ok("Accepted".into())
    }

    pub fn quick_fields() -> &'static [control_station_vehicle_state::FieldInfo] {
        FIELDS
    }

    fn fail(shared: &Shared, generation: u64, reason: String) {
        let mut inner = shared.inner.lock().expect("link state lock");
        if inner.generation != generation {
            return;
        }
        inner.generation += 1;
        // Dropping the session aborts every task, including the caller; it must return right away.
        inner.session = None;
        inner.state = LinkState::Disconnected;
        inner.error = Some(reason);
        inner.local_port = None;
        inner.dirty = true;
        let _ = shared.events.send(LinkEvent::Link(inner.info()));
        let _ = shared.events.send(LinkEvent::Telemetry(
            inner.snapshot(shared.timing.heartbeat_timeout),
        ));
    }
}

fn result_name(result: u8) -> &'static str {
    match result {
        1 => "temporarily rejected",
        2 => "denied",
        3 => "unsupported",
        4 => "failed",
        6 => "cancelled",
        _ => "unknown result",
    }
}

fn describe(config: &ConnectionConfig) -> String {
    match config {
        ConnectionConfig::Auto => "AUTO".into(),
        ConnectionConfig::Serial { port, baud } => format!("{port} @ {baud}"),
        ConnectionConfig::Tcp { host, port } => format!("TCP {host}:{port}"),
        ConnectionConfig::Udp { port } => format!("UDP :{port}"),
        ConnectionConfig::Udpcl { host, port } => format!("UDPCl {host}:{port}"),
        ConnectionConfig::Ws { url } => format!("WS {url}"),
        ConnectionConfig::Replay { path, .. } => format!("Replay {path}"),
    }
}

async fn open_config(config: &ConnectionConfig) -> Result<Connection, LinkError> {
    let endpoint = match config {
        ConnectionConfig::Auto => return scan::scan_serial().await,
        ConnectionConfig::Replay { path, speed } => {
            return replay::open(path, speed.unwrap_or(1.0)).await
        }
        ConnectionConfig::Serial { port, baud } => Endpoint::Serial {
            port: port.clone(),
            baud: *baud,
        },
        ConnectionConfig::Tcp { host, port } => Endpoint::Tcp {
            host: host.clone(),
            port: *port,
        },
        ConnectionConfig::Udp { port } => Endpoint::Udp { port: *port },
        ConnectionConfig::Udpcl { host, port } => Endpoint::UdpClient {
            host: host.clone(),
            port: *port,
        },
        ConnectionConfig::Ws { url } => Endpoint::WebSocket { url: url.clone() },
    };
    Ok(control_station_transport::open(&endpoint).await?)
}

async fn reader_task(shared: Arc<Shared>, mut reader: Box<dyn LinkReader>, generation: u64) {
    let mut parser = Parser::new();
    let mut buf = vec![0u8; 8192];
    loop {
        match reader.read(&mut buf).await {
            Ok(n) => {
                let packets = parser.push(&buf[..n]);
                let mut inner = shared.inner.lock().expect("link state lock");
                if inner.generation != generation {
                    return;
                }
                inner.counters.bytes_rx += n as u64;
                inner.counters.bad_crc = parser.stats().bad_crc;
                for packet in packets {
                    inner.handle_packet(&packet, &shared.events, &shared.acks);
                    if shared.packets.receiver_count() > 0 {
                        let _ = shared.packets.send(Arc::new(packet));
                    }
                }
            }
            Err(error) => {
                LinkManager::fail(&shared, generation, error.to_string());
                return;
            }
        }
    }
}

async fn writer_task(
    shared: Arc<Shared>,
    mut writer: Box<dyn LinkWriter>,
    mut rx: mpsc::Receiver<Vec<u8>>,
    generation: u64,
) {
    while let Some(bytes) = rx.recv().await {
        match writer.write(&bytes).await {
            Ok(()) => {
                let mut inner = shared.inner.lock().expect("link state lock");
                inner.counters.bytes_tx += bytes.len() as u64;
            }
            Err(error) => {
                LinkManager::fail(&shared, generation, error.to_string());
                return;
            }
        }
    }
}

async fn ticker_task(shared: Arc<Shared>, generation: u64) {
    let tick = shared.timing.tick;
    let ticks_per_second =
        (Duration::from_secs(1).as_millis() / tick.as_millis().max(1)).max(1) as u32;
    let mut interval = tokio::time::interval(tick);
    let mut count: u32 = 0;
    loop {
        interval.tick().await;
        count = count.wrapping_add(1);
        let once_per_second = count.is_multiple_of(ticks_per_second);
        let mut inner = shared.inner.lock().expect("link state lock");
        if inner.generation != generation {
            return;
        }
        let now = Instant::now();
        let timeout = shared.timing.heartbeat_timeout;

        let mut stream_requests = Vec::new();
        let ids: Vec<VehicleId> = inner.vehicles.keys().copied().collect();
        for id in ids {
            let alive = inner.vehicles[&id].is_alive(now, timeout);
            let requested = inner.streams_requested.contains(&id);
            if alive && !requested {
                inner.streams_requested.insert(id);
                stream_requests.extend(commands::request_data_streams(id));
                inner.dirty = true;
            } else if !alive && requested {
                inner.streams_requested.remove(&id);
                inner.dirty = true;
            }
        }
        for message in &stream_requests {
            let _ = inner.queue(message);
        }

        if once_per_second {
            let _ = inner.queue(&gcs_heartbeat());
            inner.update_rates();
            let _ = shared.events.send(LinkEvent::Link(inner.info()));
        }
        if inner.dirty {
            inner.dirty = false;
            let snapshot = inner.snapshot(timeout);
            let _ = shared.events.send(LinkEvent::Telemetry(snapshot));
        }
    }
}
