//! Vehicle parameters over MAVLink: full download with re-requests, verified writes, `.param`
//! files and metadata. Runs as a service next to [`LinkManager`], using only its public API.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_ardupilot_meta::{self as meta, ParamMeta, VehicleType};
use control_station_mavlink::{
    dialect::{MavParamType, PARAM_REQUEST_LIST_DATA, PARAM_REQUEST_READ_DATA, PARAM_SET_DATA},
    MavMessage, Packet,
};
use control_station_params::{
    format_param_file, parse_param_file, Param, ParamTable, ParamType, ParsedFile,
};
use control_station_vehicle_state::VehicleId;
use serde::Serialize;
use tokio::{
    sync::{broadcast, broadcast::error::RecvError, Mutex as AsyncMutex},
    time::{timeout_at, Instant},
};

use crate::{LinkError, LinkEvent, LinkManager, LinkState};

type Fetcher = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

#[derive(Debug, Clone, Copy)]
pub struct ParamTiming {
    /// No parameter for this long means the list request is repeated.
    pub idle_timeout: Duration,
    /// Pause between rounds of per-index reads.
    pub read_round: Duration,
    pub read_batch: usize,
    /// Rounds without a single new parameter before giving up.
    pub max_idle_rounds: u32,
    /// How long to wait for the PARAM_VALUE echo of a PARAM_SET.
    pub echo_timeout: Duration,
    pub set_attempts: u32,
}

impl Default for ParamTiming {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(4),
            read_round: Duration::from_secs(1),
            read_batch: 10,
            max_idle_rounds: 10,
            echo_timeout: Duration::from_millis(700),
            set_attempts: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamProgress {
    pub received: u32,
    pub total: u32,
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ParamError {
    #[error("{0}")]
    Link(#[from] LinkError),
    #[error("the vehicle changed or disconnected during the operation")]
    VehicleLost,
    #[error("no parameters received from the vehicle")]
    NothingReceived,
    #[error("received {received} of {total} parameters")]
    Incomplete { received: usize, total: usize },
    #[error("unknown parameter '{0}' (download the parameter list first)")]
    Unknown(String),
    #[error("parameter name '{0}' is longer than 16 characters")]
    NameTooLong(String),
    #[error("{0} did not confirm the new value (no PARAM_VALUE echo)")]
    NoEcho(String),
}

#[derive(Debug, Clone)]
struct ParamUpdate {
    name: String,
    value: f64,
    index: u16,
    count: u16,
}

#[derive(Default)]
struct State {
    /// The vehicle whose parameters the table holds; `None` until a download or write starts.
    vehicle: Option<VehicleId>,
    table: ParamTable,
}

struct Inner {
    link: LinkManager,
    state: Mutex<State>,
    updates: broadcast::Sender<ParamUpdate>,
    progress: broadcast::Sender<ParamProgress>,
    refresh_lock: AsyncMutex<()>,
    started: AtomicBool,
    timing: ParamTiming,
    cache_dir: PathBuf,
    fetcher: Fetcher,
    metadata: Mutex<HashMap<VehicleType, Arc<HashMap<String, ParamMeta>>>>,
}

#[derive(Clone)]
pub struct ParamService {
    inner: Arc<Inner>,
}

fn mav_type(param_type: ParamType) -> MavParamType {
    match param_type {
        ParamType::Uint8 => MavParamType::MAV_PARAM_TYPE_UINT8,
        ParamType::Int8 => MavParamType::MAV_PARAM_TYPE_INT8,
        ParamType::Uint16 => MavParamType::MAV_PARAM_TYPE_UINT16,
        ParamType::Int16 => MavParamType::MAV_PARAM_TYPE_INT16,
        ParamType::Uint32 => MavParamType::MAV_PARAM_TYPE_UINT32,
        ParamType::Int32 => MavParamType::MAV_PARAM_TYPE_INT32,
        ParamType::Uint64 => MavParamType::MAV_PARAM_TYPE_UINT64,
        ParamType::Int64 => MavParamType::MAV_PARAM_TYPE_INT64,
        ParamType::Real32 => MavParamType::MAV_PARAM_TYPE_REAL32,
        ParamType::Real64 => MavParamType::MAV_PARAM_TYPE_REAL64,
    }
}

impl ParamService {
    pub fn new(link: LinkManager) -> Self {
        Self::build(
            link,
            ParamTiming::default(),
            std::env::temp_dir().join("control-station-metadata"),
            Arc::new(meta::http_fetch),
        )
    }

    /// Where parsed-definition downloads are cached (the app cache directory).
    pub fn with_cache_dir(self, cache_dir: PathBuf) -> Self {
        let i = &self.inner;
        Self::build(i.link.clone(), i.timing, cache_dir, i.fetcher.clone())
    }

    pub fn with_timing(self, timing: ParamTiming) -> Self {
        let i = &self.inner;
        Self::build(
            i.link.clone(),
            timing,
            i.cache_dir.clone(),
            i.fetcher.clone(),
        )
    }

    /// Replaces the HTTPS fetcher (tests use a fake so nothing touches the network).
    pub fn with_fetcher(
        self,
        fetcher: impl Fn(&str) -> Result<Vec<u8>, String> + Send + Sync + 'static,
    ) -> Self {
        let i = &self.inner;
        Self::build(
            i.link.clone(),
            i.timing,
            i.cache_dir.clone(),
            Arc::new(fetcher),
        )
    }

    fn build(link: LinkManager, timing: ParamTiming, cache_dir: PathBuf, fetcher: Fetcher) -> Self {
        let (updates, _) = broadcast::channel(4096);
        let (progress, _) = broadcast::channel(256);
        let service = Self {
            inner: Arc::new(Inner {
                link,
                state: Mutex::new(State::default()),
                updates,
                progress,
                refresh_lock: AsyncMutex::new(()),
                started: AtomicBool::new(false),
                timing,
                cache_dir,
                fetcher,
                metadata: Mutex::new(HashMap::new()),
            }),
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            service.start();
        }
        service
    }

    /// Starts the background listener (idempotent). Needs a tokio runtime; every async method
    /// calls it, and `new` does when it is already inside a runtime.
    pub fn start(&self) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        // Subscribe before spawning so nothing sent right after `start` can be missed.
        let packets = self.inner.link.subscribe_packets();
        let events = self.inner.link.subscribe();
        tokio::spawn(listen(self.inner.clone(), packets, events));
    }

    pub fn subscribe_progress(&self) -> broadcast::Receiver<ParamProgress> {
        self.inner.progress.subscribe()
    }

    /// The downloaded parameters, naturally sorted by name.
    pub fn list(&self) -> Vec<Param> {
        self.state().table.sorted()
    }

    pub fn format_file(&self) -> String {
        format_param_file(&self.list())
    }

    pub fn parse_file(&self, text: &str) -> ParsedFile {
        parse_param_file(text)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().expect("param state lock")
    }

    fn emit(&self, received: usize, total: usize, done: bool, error: Option<String>) {
        let _ = self.inner.progress.send(ParamProgress {
            received: received as u32,
            total: total as u32,
            done,
            error,
        });
    }

    fn counts(&self) -> (usize, usize) {
        let state = self.state();
        (
            state.table.received_count(),
            usize::from(state.table.expected_count()),
        )
    }

    fn ensure_vehicle(&self, vehicle: VehicleId) -> Result<(), ParamError> {
        if self.state().vehicle == Some(vehicle) {
            Ok(())
        } else {
            Err(ParamError::VehicleLost)
        }
    }

    fn send_list_request(&self, vehicle: VehicleId) -> Result<(), ParamError> {
        self.inner
            .link
            .send(&MavMessage::PARAM_REQUEST_LIST(PARAM_REQUEST_LIST_DATA {
                target_system: vehicle.system_id,
                target_component: vehicle.component_id,
            }))?;
        Ok(())
    }

    fn send_read_request(&self, vehicle: VehicleId, index: u16) -> Result<(), ParamError> {
        self.inner
            .link
            .send(&MavMessage::PARAM_REQUEST_READ(PARAM_REQUEST_READ_DATA {
                param_index: index as i16,
                target_system: vehicle.system_id,
                target_component: vehicle.component_id,
                param_id: "".into(),
            }))?;
        Ok(())
    }

    /// Downloads every parameter of the selected vehicle and returns how many arrived.
    /// Progress is also published through [`subscribe_progress`](Self::subscribe_progress).
    pub async fn refresh(&self) -> Result<usize, ParamError> {
        self.start();
        let _only_one = self.inner.refresh_lock.lock().await;
        let result = self.download().await;
        let (received, total) = self.counts();
        match &result {
            Ok(_) => self.emit(received, total, true, None),
            Err(error) => self.emit(received, total, true, Some(error.to_string())),
        }
        result
    }

    async fn download(&self) -> Result<usize, ParamError> {
        let timing = self.inner.timing;
        let (vehicle, _) = self.inner.link.selected_vehicle()?;
        {
            let mut state = self.state();
            state.vehicle = Some(vehicle);
            state.table.clear();
        }
        let mut updates = self.inner.updates.subscribe();
        self.send_list_request(vehicle)?;
        self.emit(0, 0, false, None);
        let mut list_requests = 1;

        // Phase 1: the vehicle streams the whole list after one request.
        let mut deadline = Instant::now() + timing.idle_timeout;
        loop {
            self.ensure_vehicle(vehicle)?;
            match timeout_at(deadline, updates.recv()).await {
                Ok(Ok(update)) => {
                    deadline = Instant::now() + timing.idle_timeout;
                    let (received, total) = self.counts();
                    if received.is_multiple_of(25) {
                        self.emit(received, total, false, None);
                    }
                    if self.state().table.is_complete() {
                        return Ok(received);
                    }
                    let last_arrived = usize::from(update.index) + 1 >= usize::from(update.count);
                    if last_arrived {
                        if received * 4 >= total * 3 {
                            break;
                        }
                        if list_requests < 3 {
                            self.state().table.clear();
                            self.send_list_request(vehicle)?;
                            list_requests += 1;
                        } else {
                            break;
                        }
                    }
                }
                Ok(Err(RecvError::Lagged(_))) => {}
                Ok(Err(RecvError::Closed)) => return Err(ParamError::VehicleLost),
                Err(_idle) => {
                    let (received, total) = self.counts();
                    let mostly_there = total > 0 && received * 4 >= total * 3;
                    if !mostly_there && list_requests < 3 {
                        self.send_list_request(vehicle)?;
                        list_requests += 1;
                        deadline = Instant::now() + timing.idle_timeout;
                    } else if received == 0 {
                        return Err(ParamError::NothingReceived);
                    } else {
                        break;
                    }
                }
            }
        }

        // Phase 2: ask for what is still missing, a few indexes per round.
        let mut idle_rounds = 0;
        loop {
            self.ensure_vehicle(vehicle)?;
            let (before, total, missing) = {
                let state = self.state();
                (
                    state.table.received_count(),
                    usize::from(state.table.expected_count()),
                    state.table.missing_indices(),
                )
            };
            if missing.is_empty() {
                return Ok(before);
            }
            for index in missing.iter().take(timing.read_batch) {
                self.send_read_request(vehicle, *index)?;
            }
            tokio::time::sleep(timing.read_round).await;
            let (after, _) = self.counts();
            self.emit(after, total, false, None);
            if after == before {
                idle_rounds += 1;
                if idle_rounds >= timing.max_idle_rounds {
                    return Err(ParamError::Incomplete {
                        received: after,
                        total,
                    });
                }
            } else {
                idle_rounds = 0;
            }
        }
    }

    /// Writes one parameter and returns the value the vehicle echoed back, which can differ from
    /// the requested one (rounding, limits). Fails if the vehicle never confirms.
    pub async fn set(&self, name: &str, value: f64) -> Result<f64, ParamError> {
        self.start();
        let timing = self.inner.timing;
        if name.len() > 16 {
            return Err(ParamError::NameTooLong(name.to_owned()));
        }
        let (vehicle, _) = self.inner.link.selected_vehicle()?;
        let param_type = {
            let state = self.state();
            if state.vehicle != Some(vehicle) {
                return Err(ParamError::Unknown(name.to_owned()));
            }
            state
                .table
                .get(name)
                .map(|p| p.param_type)
                .ok_or_else(|| ParamError::Unknown(name.to_owned()))?
        };
        let mut updates = self.inner.updates.subscribe();
        for _ in 0..timing.set_attempts {
            self.inner
                .link
                .send(&MavMessage::PARAM_SET(PARAM_SET_DATA {
                    param_value: value as f32,
                    target_system: vehicle.system_id,
                    target_component: vehicle.component_id,
                    param_id: name.into(),
                    param_type: mav_type(param_type),
                }))?;
            let deadline = Instant::now() + timing.echo_timeout;
            loop {
                match timeout_at(deadline, updates.recv()).await {
                    Ok(Ok(update)) if update.name == name => return Ok(update.value),
                    Ok(Ok(_)) | Ok(Err(RecvError::Lagged(_))) => {}
                    Ok(Err(RecvError::Closed)) => return Err(ParamError::VehicleLost),
                    Err(_) => break,
                }
            }
        }
        Err(ParamError::NoEcho(name.to_owned()))
    }

    /// Definitions (descriptions, ranges, option lists) for the selected vehicle type. Downloaded
    /// once and cached on disk for a week; any failure gives an empty map.
    pub async fn metadata(&self) -> Arc<HashMap<String, ParamMeta>> {
        self.start();
        let vehicle_type = match self.inner.link.selected_vehicle() {
            Ok((_, vehicle_type)) => vehicle_type,
            Err(_) => return Arc::default(),
        };
        if let Some(cached) = self
            .inner
            .metadata
            .lock()
            .expect("metadata lock")
            .get(&vehicle_type)
        {
            return cached.clone();
        }
        let (dir, fetcher) = (self.inner.cache_dir.clone(), self.inner.fetcher.clone());
        let loaded = tokio::task::spawn_blocking(move || {
            meta::load_metadata(vehicle_type, &dir, meta::CACHE_MAX_AGE, |url| fetcher(url))
        })
        .await;
        match loaded {
            Ok(Ok(map)) => {
                let map = Arc::new(map);
                self.inner
                    .metadata
                    .lock()
                    .expect("metadata lock")
                    .insert(vehicle_type, map.clone());
                map
            }
            Ok(Err(error)) => {
                eprintln!("parameter metadata: {error}");
                Arc::default()
            }
            Err(error) => {
                eprintln!("parameter metadata task failed: {error}");
                Arc::default()
            }
        }
    }
}

async fn listen(
    inner: Arc<Inner>,
    mut packets: broadcast::Receiver<Arc<Packet>>,
    mut events: broadcast::Receiver<LinkEvent>,
) {
    loop {
        tokio::select! {
            packet = packets.recv() => match packet {
                Ok(packet) => on_packet(&inner, &packet),
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            },
            event = events.recv() => match event {
                Ok(LinkEvent::Link(info)) if info.state != LinkState::Connected => clear(&inner),
                Ok(LinkEvent::Telemetry(t)) => {
                    let mut state = inner.state.lock().expect("param state lock");
                    if state.vehicle.is_some() && state.vehicle != t.vehicle {
                        state.vehicle = None;
                        state.table.clear();
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            },
        }
    }
}

fn clear(inner: &Inner) {
    let mut state = inner.state.lock().expect("param state lock");
    state.vehicle = None;
    state.table.clear();
}

fn on_packet(inner: &Inner, packet: &Packet) {
    let MavMessage::PARAM_VALUE(p) = &packet.message else {
        return;
    };
    let bytes: &[u8] = &*p.param_id;
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let name = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let id = VehicleId {
        system_id: packet.system_id(),
        component_id: packet.component_id(),
    };
    let value = f64::from(p.param_value);
    {
        let mut state = inner.state.lock().expect("param state lock");
        if state.vehicle != Some(id) {
            return;
        }
        state.table.set_expected(p.param_count);
        let param = Param {
            name: name.clone(),
            value,
            param_type: ParamType::from_mav(p.param_type as u8),
            index: p.param_index,
        };
        if !state.table.insert(param) {
            state.table.set_value(&name, value);
        }
    }
    let _ = inner.updates.send(ParamUpdate {
        name,
        value,
        index: p.param_index,
        count: p.param_count,
    });
}
