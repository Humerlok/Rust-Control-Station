//! Sensor calibration flows: accelerometer, compass, radio, ESC, gyro and barometer.
//! A service on top of the link: it reads the packet bus and sends commands, and reports its
//! progress as [`CalibrationState`] snapshots.
// REQUEST_DATA_STREAM is superseded in the dialect but is what ArduPilot honors.
#![allow(deprecated)]

use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use control_station_commands as commands;
use control_station_mavlink::{
    dialect::{MagCalStatus, MavCmd, MavDataStream, REQUEST_DATA_STREAM_DATA},
    MavMessage, Packet,
};
use control_station_vehicle_state::VehicleId;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::{
    manager::LinkManager,
    types::{LinkError, LinkEvent, LinkState},
};

/// Writes one parameter and returns the value the vehicle echoed (supplied by the parameter service).
pub type ParamWriter = Arc<
    dyn Fn(String, f64) -> Pin<Box<dyn Future<Output = Result<f64, String>> + Send>> + Send + Sync,
>;

const ACCEL_SUCCESS: u32 = 16_777_215;
const ACCEL_FAILED: u32 = 16_777_216;
const LONG_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const RADIO_EMIT_INTERVAL: Duration = Duration::from_millis(200);
const RC_CHANNELS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationKind {
    AccelFull,
    AccelLevel,
    AccelSimple,
    Gyro,
    Baro,
    Compass,
    Radio,
    Esc,
}

impl CalibrationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::AccelFull => "accel_full",
            Self::AccelLevel => "accel_level",
            Self::AccelSimple => "accel_simple",
            Self::Gyro => "gyro",
            Self::Baro => "baro",
            Self::Compass => "compass",
            Self::Radio => "radio",
            Self::Esc => "esc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CalState {
    Idle,
    Running,
    Waiting,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccelPosition {
    pub index: u32,
    pub name: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompassProgress {
    pub compass_id: u8,
    pub completion_pct: u8,
    pub status: String,
    pub fitness: Option<f32>,
    pub autosaved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioChannel {
    pub channel: u8,
    pub min: u16,
    pub max: u16,
    pub value: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationState {
    pub kind: Option<&'static str>,
    pub state: CalState,
    pub message: String,
    pub accel_position: Option<AccelPosition>,
    pub compass: Vec<CompassProgress>,
    pub radio: Vec<RadioChannel>,
}

/// Min/max/current of the RC inputs while the user moves every stick and switch.
#[derive(Debug, Clone)]
pub struct RadioCal {
    min: [u16; RC_CHANNELS],
    max: [u16; RC_CHANNELS],
    value: [u16; RC_CHANNELS],
}

impl RadioCal {
    pub fn new() -> Self {
        Self {
            min: [3000; RC_CHANNELS],
            max: [0; RC_CHANNELS],
            value: [0; RC_CHANNELS],
        }
    }

    /// 0 and 65535 mean "channel not provided".
    pub fn update(&mut self, raw: &[u16; RC_CHANNELS]) {
        for (i, v) in raw.iter().copied().enumerate() {
            if v == 0 || v == u16::MAX {
                continue;
            }
            self.value[i] = v;
            self.min[i] = self.min[i].min(v);
            self.max[i] = self.max[i].max(v);
        }
    }

    pub fn channels(&self) -> Vec<RadioChannel> {
        (0..RC_CHANNELS)
            .map(|i| RadioChannel {
                channel: (i + 1) as u8,
                min: if self.max[i] == 0 { 0 } else { self.min[i] },
                max: self.max[i],
                value: self.value[i],
            })
            .collect()
    }

    /// The `RCn_MIN/MAX/TRIM` writes the legacy tool performs. Trim is the current position
    /// (sticks centered, throttle down) constrained to the observed range.
    pub fn plan(&self) -> Result<Vec<(String, f64)>, String> {
        if !(800..2200).contains(&self.min[0]) || self.max[0] == 0 {
            return Err("bad channel 1 input, nothing was written".into());
        }
        let mut writes = Vec::new();
        for i in 0..RC_CHANNELS {
            let (min, max) = (self.min[i], self.max[i]);
            let trim = self.value[i].clamp(min.min(max), max);
            if min < max && min != 0 && max != 0 && trim != 0 {
                let n = i + 1;
                writes.push((format!("RC{n}_MIN"), f64::from(min)));
                writes.push((format!("RC{n}_MAX"), f64::from(max)));
                writes.push((format!("RC{n}_TRIM"), f64::from(trim)));
            }
        }
        Ok(writes)
    }
}

impl Default for RadioCal {
    fn default() -> Self {
        Self::new()
    }
}

struct Inner {
    kind: Option<CalibrationKind>,
    state: CalState,
    message: String,
    accel_position: Option<AccelPosition>,
    last_accel_position: Option<u32>,
    compass: BTreeMap<u8, CompassProgress>,
    compass_mask: u8,
    radio: RadioCal,
    last_radio_emit: Instant,
}

impl Inner {
    fn idle() -> Self {
        Self {
            kind: None,
            state: CalState::Idle,
            message: String::new(),
            accel_position: None,
            last_accel_position: None,
            compass: BTreeMap::new(),
            compass_mask: 0,
            radio: RadioCal::new(),
            last_radio_emit: Instant::now(),
        }
    }

    fn busy(&self) -> bool {
        matches!(self.state, CalState::Running | CalState::Waiting)
    }

    fn snapshot(&self) -> CalibrationState {
        CalibrationState {
            kind: self.kind.map(CalibrationKind::as_str),
            state: self.state,
            message: self.message.clone(),
            accel_position: self.accel_position.clone(),
            compass: self.compass.values().cloned().collect(),
            radio: if self.kind == Some(CalibrationKind::Radio) {
                self.radio.channels()
            } else {
                Vec::new()
            },
        }
    }
}

type Receivers = (
    broadcast::Receiver<Arc<Packet>>,
    broadcast::Receiver<LinkEvent>,
);

struct Shared {
    link: LinkManager,
    params: ParamWriter,
    inner: Mutex<Inner>,
    events: broadcast::Sender<CalibrationState>,
    /// Subscribed at construction so nothing is missed before `run` is scheduled.
    receivers: Mutex<Option<Receivers>>,
}

#[derive(Clone)]
pub struct CalibrationService {
    shared: Arc<Shared>,
}

fn position_name(position: u32) -> Option<&'static str> {
    match position {
        1 => Some("Level"),
        2 => Some("Left"),
        3 => Some("Right"),
        4 => Some("Nose down"),
        5 => Some("Nose up"),
        6 => Some("Back"),
        _ => None,
    }
}

fn compass_status(status: MagCalStatus) -> &'static str {
    match status {
        MagCalStatus::MAG_CAL_NOT_STARTED => "not started",
        MagCalStatus::MAG_CAL_WAITING_TO_START => "waiting to start",
        MagCalStatus::MAG_CAL_RUNNING_STEP_ONE => "step one",
        MagCalStatus::MAG_CAL_RUNNING_STEP_TWO => "step two",
        MagCalStatus::MAG_CAL_SUCCESS => "success",
        MagCalStatus::MAG_CAL_FAILED => "failed",
        MagCalStatus::MAG_CAL_FAILED_ORIENTATION => "bad orientation",
        MagCalStatus::MAG_CAL_FAILED_RADIUS => "bad radius",
        MagCalStatus::MAG_CAL_FAILED_OFFSETS => "bad offsets",
        MagCalStatus::MAG_CAL_FAILED_DIAG_SCALING => "bad scaling",
        _ => "failed",
    }
}

fn is_compass_final(status: MagCalStatus) -> bool {
    !matches!(
        status,
        MagCalStatus::MAG_CAL_NOT_STARTED
            | MagCalStatus::MAG_CAL_WAITING_TO_START
            | MagCalStatus::MAG_CAL_RUNNING_STEP_ONE
            | MagCalStatus::MAG_CAL_RUNNING_STEP_TWO
    )
}

impl CalibrationService {
    pub fn new(link: LinkManager, params: ParamWriter) -> Self {
        let (events, _) = broadcast::channel(256);
        let receivers = (link.subscribe_packets(), link.subscribe());
        Self {
            shared: Arc::new(Shared {
                link,
                params,
                inner: Mutex::new(Inner::idle()),
                events,
                receivers: Mutex::new(Some(receivers)),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CalibrationState> {
        self.shared.events.subscribe()
    }

    pub fn status(&self) -> CalibrationState {
        self.lock().snapshot()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.shared.inner.lock().expect("calibration state lock")
    }

    fn emit(&self, inner: &Inner) {
        let _ = self.shared.events.send(inner.snapshot());
    }

    fn update(&self, change: impl FnOnce(&mut Inner)) {
        let mut inner = self.lock();
        change(&mut inner);
        self.emit(&inner);
    }

    /// Consumes the packet bus until the link manager goes away. Spawn it once; a second call
    /// returns immediately.
    pub async fn run(&self) {
        let Some((mut packets, mut events)) = self
            .shared
            .receivers
            .lock()
            .expect("calibration receivers lock")
            .take()
        else {
            return;
        };
        loop {
            tokio::select! {
                packet = packets.recv() => match packet {
                    Ok(packet) => self.handle_packet(&packet),
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                event = events.recv() => match event {
                    Ok(LinkEvent::Link(info)) if info.state != LinkState::Connected => self.reset(),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
            }
        }
    }

    fn reset(&self) {
        let mut inner = self.lock();
        if inner.kind.is_some() {
            *inner = Inner::idle();
            self.emit(&inner);
        }
    }

    /// Applies one packet to the active calibration (public for tests).
    pub fn handle_packet(&self, packet: &Packet) {
        let mut inner = self.lock();
        let Some(kind) = inner.kind else { return };
        match &packet.message {
            MavMessage::COMMAND_LONG(c)
                if c.command == MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS
                    && matches!(
                        kind,
                        CalibrationKind::AccelFull | CalibrationKind::AccelSimple
                    )
                    && inner.busy() =>
            {
                let position = c.param1 as u32;
                match position {
                    ACCEL_SUCCESS => {
                        inner.state = CalState::Succeeded;
                        inner.accel_position = None;
                        inner.message =
                            "Accelerometer calibration successful. Reboot the vehicle.".into();
                    }
                    ACCEL_FAILED => {
                        inner.state = CalState::Failed;
                        inner.accel_position = None;
                        inner.message = "Accelerometer calibration failed.".into();
                    }
                    p => {
                        let Some(name) = position_name(p) else { return };
                        inner.state = CalState::Waiting;
                        inner.last_accel_position = Some(p);
                        inner.accel_position = Some(AccelPosition { index: p, name });
                        inner.message = format!("Place the vehicle {name} and press Done.");
                    }
                }
                self.emit(&inner);
            }
            MavMessage::STATUSTEXT(t)
                if matches!(
                    kind,
                    CalibrationKind::AccelFull | CalibrationKind::AccelSimple
                ) && inner.busy() =>
            {
                let bytes: &[u8] = &*t.text;
                let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                let text = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                if text.contains("calibration successful") {
                    inner.state = CalState::Succeeded;
                    inner.accel_position = None;
                    inner.message =
                        "Accelerometer calibration successful. Reboot the vehicle.".into();
                    self.emit(&inner);
                } else if text.contains("calibration failed") {
                    inner.state = CalState::Failed;
                    inner.accel_position = None;
                    inner.message = "Accelerometer calibration failed.".into();
                    self.emit(&inner);
                }
            }
            MavMessage::MAG_CAL_PROGRESS(p) if kind == CalibrationKind::Compass && inner.busy() => {
                inner.compass_mask |= p.cal_mask;
                let entry = inner
                    .compass
                    .entry(p.compass_id)
                    .or_insert_with(|| CompassProgress {
                        compass_id: p.compass_id,
                        completion_pct: 0,
                        status: String::new(),
                        fitness: None,
                        autosaved: false,
                    });
                entry.completion_pct = p.completion_pct;
                entry.status = compass_status(p.cal_status).to_owned();
                inner.state = CalState::Running;
                inner.message = "Rotate the vehicle slowly around all axes.".into();
                self.emit(&inner);
            }
            MavMessage::MAG_CAL_REPORT(r) if kind == CalibrationKind::Compass && inner.busy() => {
                inner.compass_mask |= r.cal_mask;
                let finished = is_compass_final(r.cal_status);
                let success = r.cal_status == MagCalStatus::MAG_CAL_SUCCESS;
                let entry = inner
                    .compass
                    .entry(r.compass_id)
                    .or_insert_with(|| CompassProgress {
                        compass_id: r.compass_id,
                        completion_pct: 0,
                        status: String::new(),
                        fitness: None,
                        autosaved: false,
                    });
                entry.status = compass_status(r.cal_status).to_owned();
                entry.fitness = Some(r.fitness);
                entry.autosaved = r.autosaved != 0;
                if success {
                    entry.completion_pct = 100;
                }
                // Compass `i` is bit `i` of the calibration mask; without a mask, wait for the ones seen.
                let mask = inner.compass_mask;
                let all_done = if mask == 0 {
                    inner.compass.values().all(|c| c.fitness.is_some())
                } else {
                    (0..8u8)
                        .filter(|i| mask & (1 << i) != 0)
                        .all(|i| inner.compass.get(&i).is_some_and(|c| c.fitness.is_some()))
                };
                if finished && all_done {
                    let all_ok = inner.compass.values().all(|c| c.status == "success");
                    if all_ok {
                        inner.state = CalState::Succeeded;
                        let saved = inner.compass.values().all(|c| c.autosaved);
                        inner.message = if saved {
                            "Compass calibration saved. Reboot the vehicle.".into()
                        } else {
                            "Compass calibration finished. Accept it to save, then reboot.".into()
                        };
                    } else {
                        inner.state = CalState::Failed;
                        inner.message =
                            "Compass calibration failed. Move away from metal and retry.".into();
                    }
                }
                self.emit(&inner);
            }
            MavMessage::RC_CHANNELS(r) if kind == CalibrationKind::Radio && inner.busy() => {
                inner.radio.update(&[
                    r.chan1_raw,
                    r.chan2_raw,
                    r.chan3_raw,
                    r.chan4_raw,
                    r.chan5_raw,
                    r.chan6_raw,
                    r.chan7_raw,
                    r.chan8_raw,
                    r.chan9_raw,
                    r.chan10_raw,
                    r.chan11_raw,
                    r.chan12_raw,
                    r.chan13_raw,
                    r.chan14_raw,
                    r.chan15_raw,
                    r.chan16_raw,
                ]);
                if inner.last_radio_emit.elapsed() >= RADIO_EMIT_INTERVAL {
                    inner.last_radio_emit = Instant::now();
                    self.emit(&inner);
                }
            }
            _ => {}
        }
    }

    fn target(&self) -> Result<VehicleId, LinkError> {
        let (id, _) = self.shared.link.selected_vehicle()?;
        if self.shared.link.telemetry().armed {
            return Err(LinkError::Other(
                "the vehicle is armed; disarm before calibrating".into(),
            ));
        }
        Ok(id)
    }

    fn begin(&self, kind: CalibrationKind, message: &str) -> Result<(), LinkError> {
        let mut inner = self.lock();
        if inner.busy() {
            return Err(LinkError::Other(
                "a calibration is already in progress; cancel it first".into(),
            ));
        }
        *inner = Inner::idle();
        inner.kind = Some(kind);
        inner.state = CalState::Running;
        inner.message = message.into();
        self.emit(&inner);
        Ok(())
    }

    fn finish(&self, state: CalState, message: impl Into<String>) {
        self.update(|inner| {
            inner.state = state;
            inner.message = message.into();
        });
    }

    fn preflight(id: VehicleId, p: [f32; 7]) -> MavMessage {
        commands::command_long(id, MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION, p)
    }

    pub async fn start(&self, kind: CalibrationKind) -> Result<String, LinkError> {
        let id = self.target()?;
        let link = &self.shared.link;
        match kind {
            CalibrationKind::AccelFull | CalibrationKind::AccelSimple => {
                self.begin(kind, "Starting accelerometer calibration...")?;
                let p5 = if kind == CalibrationKind::AccelFull {
                    1.0
                } else {
                    4.0
                };
                if let Err(error) =
                    link.send(&Self::preflight(id, [0.0, 0.0, 0.0, 0.0, p5, 0.0, 0.0]))
                {
                    self.finish(CalState::Failed, error.to_string());
                    return Err(error);
                }
                Ok("Accelerometer calibration started".into())
            }
            CalibrationKind::AccelLevel => {
                self.run_long(
                    kind,
                    "Keep the vehicle level and still...",
                    Self::preflight(id, [0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
                    "Level calibration complete.",
                )
                .await
            }
            CalibrationKind::Gyro => {
                self.run_long(
                    kind,
                    "Keep the vehicle completely still...",
                    Self::preflight(id, [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                    "Gyro calibration complete.",
                )
                .await
            }
            CalibrationKind::Baro => {
                self.run_long(
                    kind,
                    "Calibrating the barometer...",
                    Self::preflight(id, [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]),
                    "Barometer calibration complete.",
                )
                .await
            }
            CalibrationKind::Compass => {
                self.begin(kind, "Starting compass calibration...")?;
                let start = commands::command_long(
                    id,
                    MavCmd::MAV_CMD_DO_START_MAG_CAL,
                    [0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
                );
                if let Err(error) = link.send_command(start).await {
                    self.finish(CalState::Failed, error.to_string());
                    return Err(error);
                }
                Ok("Compass calibration started".into())
            }
            CalibrationKind::Radio => {
                self.begin(
                    kind,
                    "Move every stick and switch to its extremes, then press Finish.",
                )?;
                self.request_rc_stream(id, 10);
                Ok("Radio calibration started".into())
            }
            CalibrationKind::Esc => {
                self.begin(kind, "Setting ESC_CALIBRATION...")?;
                match (self.shared.params)("ESC_CALIBRATION".into(), 3.0).await {
                    Ok(_) => {
                        let message = "ESC_CALIBRATION is set to 3. Remove the props, disconnect the USB \
                            and battery, then power the vehicle with the battery to start the ESC calibration.";
                        self.finish(CalState::Succeeded, message);
                        Ok(message.into())
                    }
                    Err(error) => {
                        self.finish(
                            CalState::Failed,
                            format!("could not set ESC_CALIBRATION: {error}"),
                        );
                        Err(LinkError::Other(error))
                    }
                }
            }
        }
    }

    /// Commands that only answer when the operation is over.
    async fn run_long(
        &self,
        kind: CalibrationKind,
        running: &str,
        command: MavMessage,
        done: &str,
    ) -> Result<String, LinkError> {
        self.begin(kind, running)?;
        match self
            .shared
            .link
            .send_command_with(command, LONG_COMMAND_TIMEOUT, 1)
            .await
        {
            Ok(()) => {
                self.finish(CalState::Succeeded, done);
                Ok(done.into())
            }
            Err(error) => {
                self.finish(CalState::Failed, error.to_string());
                Err(error)
            }
        }
    }

    /// The user placed the vehicle in the position the vehicle asked for.
    pub fn accel_confirm(&self) -> Result<String, LinkError> {
        let position = {
            let inner = self.lock();
            if inner.state != CalState::Waiting {
                return Err(LinkError::Other(
                    "the vehicle is not waiting for a position".into(),
                ));
            }
            inner.last_accel_position
        }
        .ok_or_else(|| LinkError::Other("no position requested yet".into()))?;
        let (id, _) = self.shared.link.selected_vehicle()?;
        self.shared.link.send(&commands::command_long(
            id,
            MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS,
            [position as f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ))?;
        self.update(|inner| {
            inner.state = CalState::Running;
            inner.message = "Hold still while the vehicle samples this position...".into();
        });
        Ok("Position confirmed".into())
    }

    pub async fn compass_accept(&self) -> Result<String, LinkError> {
        let (id, _) = self.shared.link.selected_vehicle()?;
        self.shared
            .link
            .send_command(commands::command_long(
                id,
                MavCmd::MAV_CMD_DO_ACCEPT_MAG_CAL,
                [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            ))
            .await?;
        self.finish(
            CalState::Succeeded,
            "Compass calibration accepted. Reboot the vehicle.",
        );
        Ok("Accepted".into())
    }

    pub async fn compass_cancel(&self) -> Result<String, LinkError> {
        let (id, _) = self.shared.link.selected_vehicle()?;
        self.shared
            .link
            .send_command(commands::command_long(
                id,
                MavCmd::MAV_CMD_DO_CANCEL_MAG_CAL,
                [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            ))
            .await?;
        self.reset_to_idle("Compass calibration cancelled.");
        Ok("Cancelled".into())
    }

    /// Stops whatever is running on the GCS side (and cancels a compass calibration).
    pub async fn cancel(&self) -> Result<String, LinkError> {
        let kind = self.lock().kind;
        if kind == Some(CalibrationKind::Compass) {
            return self.compass_cancel().await;
        }
        if kind == Some(CalibrationKind::Radio) {
            if let Ok((id, _)) = self.shared.link.selected_vehicle() {
                self.request_rc_stream(id, 2);
            }
        }
        self.reset_to_idle("Calibration cancelled.");
        Ok("Cancelled".into())
    }

    fn reset_to_idle(&self, message: &str) {
        let mut inner = self.lock();
        *inner = Inner::idle();
        inner.message = message.into();
        self.emit(&inner);
    }

    /// Writes RCn_MIN/MAX/TRIM for every channel that moved.
    pub async fn radio_finish(&self) -> Result<String, LinkError> {
        let plan = {
            let inner = self.lock();
            if inner.kind != Some(CalibrationKind::Radio) || !inner.busy() {
                return Err(LinkError::Other("no radio calibration is running".into()));
            }
            inner.radio.plan()
        };
        let writes = plan.map_err(LinkError::Other)?;
        let mut failures = Vec::new();
        for (name, value) in &writes {
            if let Err(error) = (self.shared.params)(name.clone(), *value).await {
                failures.push(format!("{name}: {error}"));
            }
        }
        if let Ok((id, _)) = self.shared.link.selected_vehicle() {
            self.request_rc_stream(id, 2);
        }
        let channels = writes.len() / 3;
        if failures.is_empty() {
            let message = format!("Radio calibration saved for {channels} channels.");
            self.finish(CalState::Succeeded, message.clone());
            Ok(message)
        } else {
            let message = format!("Some parameters were not written: {}", failures.join("; "));
            self.finish(CalState::Failed, message.clone());
            Err(LinkError::Other(message))
        }
    }

    fn request_rc_stream(&self, id: VehicleId, rate: u16) {
        let _ = self
            .shared
            .link
            .send(&MavMessage::REQUEST_DATA_STREAM(REQUEST_DATA_STREAM_DATA {
                req_message_rate: rate,
                target_system: id.system_id,
                target_component: id.component_id,
                req_stream_id: MavDataStream::MAV_DATA_STREAM_RC_CHANNELS,
                start_stop: 1,
            }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_station_mavlink::{
        dialect::{COMMAND_LONG_DATA, MAG_CAL_REPORT_DATA, STATUSTEXT_DATA},
        MavHeader, MavlinkVersion,
    };

    fn service() -> CalibrationService {
        let writer: ParamWriter = Arc::new(|_, v| Box::pin(async move { Ok(v) }));
        CalibrationService::new(LinkManager::new(), writer)
    }

    fn packet(message: MavMessage) -> Packet {
        Packet {
            header: MavHeader {
                system_id: 1,
                component_id: 1,
                sequence: 0,
            },
            message,
            version: MavlinkVersion::V2,
            wire_len: 0,
        }
    }

    fn accel_position(p: u32) -> Packet {
        packet(MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
            command: MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS,
            param1: p as f32,
            ..COMMAND_LONG_DATA::default()
        }))
    }

    fn start(service: &CalibrationService, kind: CalibrationKind) {
        service.begin(kind, "test").unwrap();
    }

    #[test]
    fn accel_positions_then_success() {
        let s = service();
        start(&s, CalibrationKind::AccelFull);
        s.handle_packet(&accel_position(1));
        let st = s.status();
        assert_eq!(st.state, CalState::Waiting);
        assert_eq!(st.accel_position.unwrap().name, "Level");
        s.handle_packet(&accel_position(4));
        assert_eq!(s.status().accel_position.unwrap().name, "Nose down");
        s.handle_packet(&accel_position(ACCEL_SUCCESS));
        let st = s.status();
        assert_eq!(st.state, CalState::Succeeded);
        assert!(st.accel_position.is_none());
    }

    #[test]
    fn accel_failure_code_fails() {
        let s = service();
        start(&s, CalibrationKind::AccelSimple);
        s.handle_packet(&accel_position(ACCEL_FAILED));
        assert_eq!(s.status().state, CalState::Failed);
    }

    #[test]
    fn accel_statustext_fallback() {
        let s = service();
        start(&s, CalibrationKind::AccelFull);
        s.handle_packet(&packet(MavMessage::STATUSTEXT(STATUSTEXT_DATA {
            text: "Calibration successful".into(),
            ..STATUSTEXT_DATA::default()
        })));
        assert_eq!(s.status().state, CalState::Succeeded);
    }

    #[test]
    fn packets_are_ignored_when_idle_or_for_another_kind() {
        let s = service();
        s.handle_packet(&accel_position(1));
        assert_eq!(s.status().state, CalState::Idle);
        start(&s, CalibrationKind::Compass);
        s.handle_packet(&accel_position(1));
        assert_eq!(s.status().state, CalState::Running);
        assert!(s.status().accel_position.is_none());
    }

    fn compass_report(id: u8, mask: u8, status: MagCalStatus, autosaved: u8) -> Packet {
        packet(MavMessage::MAG_CAL_REPORT(MAG_CAL_REPORT_DATA {
            compass_id: id,
            cal_mask: mask,
            cal_status: status,
            autosaved,
            fitness: 8.5,
            ..MAG_CAL_REPORT_DATA::default()
        }))
    }

    #[test]
    fn compass_reports_finish_when_every_compass_reported() {
        let s = service();
        start(&s, CalibrationKind::Compass);
        s.handle_packet(&compass_report(0, 0b11, MagCalStatus::MAG_CAL_SUCCESS, 1));
        assert_eq!(
            s.status().state,
            CalState::Running,
            "second compass still pending"
        );
        s.handle_packet(&compass_report(1, 0b11, MagCalStatus::MAG_CAL_SUCCESS, 1));
        let st = s.status();
        assert_eq!(st.state, CalState::Succeeded);
        assert!(st.message.contains("saved"));
        assert_eq!(st.compass.len(), 2);
        assert_eq!(st.compass[0].fitness, Some(8.5));
    }

    #[test]
    fn a_failed_compass_fails_the_calibration() {
        let s = service();
        start(&s, CalibrationKind::Compass);
        s.handle_packet(&compass_report(0, 0b1, MagCalStatus::MAG_CAL_FAILED, 0));
        assert_eq!(s.status().state, CalState::Failed);
    }

    #[test]
    fn radio_tracks_extremes_and_plans_writes() {
        let mut radio = RadioCal::new();
        let mut raw = [u16::MAX; RC_CHANNELS];
        for (i, v) in [(0usize, 1500u16), (1, 1500), (2, 1000), (3, 1500)] {
            raw[i] = v;
        }
        radio.update(&raw);
        raw[0] = 1100;
        raw[1] = 1900;
        raw[2] = 2000;
        radio.update(&raw);
        raw[0] = 1500;
        raw[1] = 1500;
        raw[2] = 1000;
        radio.update(&raw);
        let channels = radio.channels();
        assert_eq!(
            (channels[0].min, channels[0].max, channels[0].value),
            (1100, 1500, 1500)
        );
        assert_eq!(channels[5].max, 0, "untouched channels stay empty");
        let plan = radio.plan().expect("channel 1 moved within a sane range");
        assert!(plan.iter().any(|(n, v)| n == "RC1_MIN" && *v == 1100.0));
    }

    #[test]
    fn radio_plan_skips_unmoved_channels_and_constrains_trim() {
        let mut radio = RadioCal::new();
        let mut raw = [u16::MAX; RC_CHANNELS];
        raw[0] = 1100;
        raw[2] = 1000;
        radio.update(&raw);
        raw[0] = 1900;
        raw[2] = 2000;
        radio.update(&raw);
        raw[0] = 1510;
        raw[2] = 1000;
        radio.update(&raw);
        let plan = radio.plan().unwrap();
        let get = |name: &str| plan.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
        assert_eq!(get("RC1_MIN"), Some(1100.0));
        assert_eq!(get("RC1_MAX"), Some(1900.0));
        assert_eq!(get("RC1_TRIM"), Some(1510.0));
        assert_eq!(get("RC3_TRIM"), Some(1000.0));
        assert_eq!(get("RC2_MIN"), None);
        assert_eq!(plan.len(), 6);
    }

    #[test]
    fn radio_plan_rejects_a_dead_channel_one() {
        assert!(RadioCal::new().plan().is_err());
    }

    #[test]
    fn state_serializes_to_the_ui_contract() {
        let s = service();
        start(&s, CalibrationKind::Radio);
        let value = serde_json::to_value(s.status()).unwrap();
        assert_eq!(value["kind"], "radio");
        assert_eq!(value["state"], "running");
        assert_eq!(value["radio"].as_array().unwrap().len(), 16);
        assert!(value.get("accelPosition").is_some());
        assert_eq!(value["radio"][0]["channel"], 1);
    }
}
