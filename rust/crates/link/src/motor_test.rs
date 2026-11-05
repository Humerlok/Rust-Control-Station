//! Motor test: spin one motor, all of them, or the whole sequence with `MAV_CMD_DO_MOTOR_TEST`,
//! and show the ESC telemetry the vehicle reports meanwhile. Every safety rule lives here: the
//! vehicle must be disarmed, the throttle is bounded, and only one test runs at a time. Stopping
//! is always allowed.
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use control_station_ardupilot_meta::{fallback_motor_count, motor_layout};
use control_station_commands::{self as commands, MotorThrottleType};
use control_station_mavlink::{MavMessage, Packet};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::{
    calibration::ParamWriter,
    manager::LinkManager,
    types::{LinkError, LinkEvent, LinkState},
};

/// Above this percentage the caller has to say it is on purpose (`allow_high`).
pub const HIGH_THROTTLE_PERCENT: f32 = 20.0;
/// The longest single test the screen may ask for, in seconds.
pub const MAX_SECONDS: f32 = 120.0;
/// The `MOT_SPIN_ARM` / `MOT_SPIN_MIN` values (percent) the screen may write.
pub const MAX_SPIN_PERCENT: f32 = 30.0;
const MAX_MOTORS: u8 = 12;
const EMIT_INTERVAL: Duration = Duration::from_millis(250);
const ESC_STALE: Duration = Duration::from_secs(3);
const ESC_STREAM_INTERVAL_US: i32 = 250_000;

const MSG_ESC_STATUS: u32 = 291;
const MSG_ESC_TELEMETRY_1_TO_4: u32 = 11030;
const MSG_ESC_TELEMETRY_5_TO_8: u32 = 11031;
const MSG_ESC_TELEMETRY_9_TO_12: u32 = 11032;
const ESC_MESSAGES: [u32; 4] = [
    MSG_ESC_STATUS,
    MSG_ESC_TELEMETRY_1_TO_4,
    MSG_ESC_TELEMETRY_5_TO_8,
    MSG_ESC_TELEMETRY_9_TO_12,
];

/// One motor test as the screen asks for it.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotorTestRequest {
    /// 1-based position in the test order (motor A is 1). Ignored by "all" and "sequence".
    #[serde(default)]
    pub motor: u8,
    pub throttle_type: MotorThrottleType,
    /// Percent for `Percent`, microseconds for `Pwm`, unused for `Pilot`.
    #[serde(default)]
    pub throttle: f32,
    /// How long each motor spins (the delay between motors in a sequence).
    pub seconds: f32,
    #[serde(default)]
    pub allow_high: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpinParam {
    /// `MOT_SPIN_ARM`: output when armed but on the ground.
    Arm,
    /// `MOT_SPIN_MIN`: lowest output in flight.
    Min,
}

impl SpinParam {
    fn name(self) -> &'static str {
        match self {
            Self::Arm => "MOT_SPIN_ARM",
            Self::Min => "MOT_SPIN_MIN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotorSlot {
    /// "A", "B", "C"...
    pub label: String,
    /// The `motor` value that tests it.
    pub test_order: u8,
    /// The output number, when the frame table knows it.
    pub output: Option<u8>,
    /// "CW" or "CCW", when known.
    pub rotation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotorFrame {
    /// False when the frame has no table entry and the motors are generic.
    pub known: bool,
    pub class_name: Option<String>,
    pub type_name: Option<String>,
    pub motors: Vec<MotorSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EscReading {
    /// 1-based ESC index.
    pub index: u8,
    pub rpm: Option<i32>,
    pub voltage: Option<f32>,
    pub current: Option<f32>,
    pub temperature: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotorTestState {
    pub escs: Vec<EscReading>,
    /// A test is believed to be spinning (it ends by itself after its duration).
    pub running: bool,
    pub message: String,
}

struct Inner {
    escs: Vec<(EscReading, Instant)>,
    busy_until: Option<Instant>,
    message: String,
    dirty: bool,
    last_running: bool,
}

impl Inner {
    fn new() -> Self {
        Self {
            escs: Vec::new(),
            busy_until: None,
            message: String::new(),
            dirty: false,
            last_running: false,
        }
    }

    fn running(&self) -> bool {
        self.busy_until.is_some_and(|until| until > Instant::now())
    }

    fn snapshot(&self) -> MotorTestState {
        let mut escs: Vec<EscReading> = self.escs.iter().map(|(e, _)| e.clone()).collect();
        escs.sort_by_key(|e| e.index);
        MotorTestState {
            escs,
            running: self.running(),
            message: self.message.clone(),
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
    events: broadcast::Sender<MotorTestState>,
    receivers: Mutex<Option<Receivers>>,
}

#[derive(Clone)]
pub struct MotorTestService {
    shared: Arc<Shared>,
}

fn error(text: impl Into<String>) -> LinkError {
    LinkError::Other(text.into())
}

/// Rejects a request the screen should never send: out-of-range throttle or duration, or a
/// high throttle that was not confirmed.
pub fn validate(request: &MotorTestRequest) -> Result<(), LinkError> {
    if !request.seconds.is_finite() || request.seconds <= 0.0 || request.seconds > MAX_SECONDS {
        return Err(error(format!(
            "the duration must be between 0 and {MAX_SECONDS} seconds"
        )));
    }
    match request.throttle_type {
        MotorThrottleType::Percent => {
            let t = request.throttle;
            if !t.is_finite() || !(0.0..=100.0).contains(&t) {
                return Err(error("the throttle must be between 0 and 100 %"));
            }
            if t > HIGH_THROTTLE_PERCENT && !request.allow_high {
                return Err(error(format!(
                    "{t:.0} % is above {HIGH_THROTTLE_PERCENT:.0} %: confirm the high throttle first"
                )));
            }
        }
        MotorThrottleType::Pwm => {
            let t = request.throttle;
            if !t.is_finite() || !(800.0..=2200.0).contains(&t) {
                return Err(error("the PWM must be between 800 and 2200 µs"));
            }
        }
        MotorThrottleType::Pilot => {}
    }
    Ok(())
}

fn label(order: u8) -> String {
    char::from(b'A' + (order - 1) % 26).to_string()
}

impl MotorTestService {
    pub fn new(link: LinkManager, params: ParamWriter) -> Self {
        let (events, _) = broadcast::channel(64);
        let receivers = (link.subscribe_packets(), link.subscribe());
        Self {
            shared: Arc::new(Shared {
                link,
                params,
                inner: Mutex::new(Inner::new()),
                events,
                receivers: Mutex::new(Some(receivers)),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MotorTestState> {
        self.shared.events.subscribe()
    }

    pub fn status(&self) -> MotorTestState {
        self.lock().snapshot()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.shared.inner.lock().expect("motor test state lock")
    }

    fn emit(&self, inner: &mut Inner) {
        inner.dirty = false;
        inner.last_running = inner.running();
        let _ = self.shared.events.send(inner.snapshot());
    }

    fn update(&self, change: impl FnOnce(&mut Inner)) {
        let mut inner = self.lock();
        change(&mut inner);
        self.emit(&mut inner);
    }

    /// Consumes the packet bus until the link manager goes away. Spawn it once; a second call
    /// returns immediately.
    pub async fn run(&self) {
        let Some((mut packets, mut events)) = self
            .shared
            .receivers
            .lock()
            .expect("motor test receivers lock")
            .take()
        else {
            return;
        };
        let mut tick = tokio::time::interval(EMIT_INTERVAL);
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
                _ = tick.tick() => self.on_tick(),
            }
        }
    }

    fn reset(&self) {
        let mut inner = self.lock();
        *inner = Inner::new();
        self.emit(&mut inner);
    }

    /// Emits when something changed: new readings, a reading that went stale, or the end of a test.
    fn on_tick(&self) {
        let mut inner = self.lock();
        let before = inner.escs.len();
        inner.escs.retain(|(_, seen)| seen.elapsed() < ESC_STALE);
        if inner.escs.len() != before {
            inner.dirty = true;
        }
        if inner.running() != inner.last_running {
            inner.dirty = true;
        }
        if inner.dirty {
            self.emit(&mut inner);
        }
    }

    /// Applies one packet: ESC telemetry fills the readings (public for tests).
    pub fn handle_packet(&self, packet: &Packet) {
        let mut readings: Vec<EscReading> = Vec::new();
        match &packet.message {
            MavMessage::ESC_TELEMETRY_1_TO_4(m) => collect(
                &mut readings,
                1,
                &m.rpm,
                &m.voltage,
                &m.current,
                &m.temperature,
            ),
            MavMessage::ESC_TELEMETRY_5_TO_8(m) => collect(
                &mut readings,
                5,
                &m.rpm,
                &m.voltage,
                &m.current,
                &m.temperature,
            ),
            MavMessage::ESC_TELEMETRY_9_TO_12(m) => collect(
                &mut readings,
                9,
                &m.rpm,
                &m.voltage,
                &m.current,
                &m.temperature,
            ),
            MavMessage::ESC_STATUS(m) => {
                for i in 0..4 {
                    let reading = EscReading {
                        index: m.index.saturating_add(1).saturating_add(i as u8),
                        rpm: Some(m.rpm[i]),
                        voltage: Some(m.voltage[i]),
                        current: Some(m.current[i]),
                        temperature: None,
                    };
                    if m.rpm[i] != 0 || m.voltage[i] != 0.0 || m.current[i] != 0.0 {
                        readings.push(reading);
                    }
                }
            }
            _ => return,
        }
        if readings.is_empty() {
            return;
        }
        let mut inner = self.lock();
        let now = Instant::now();
        for reading in readings {
            match inner
                .escs
                .iter_mut()
                .find(|(e, _)| e.index == reading.index)
            {
                Some(slot) => *slot = (reading, now),
                None => inner.escs.push((reading, now)),
            }
        }
        inner.dirty = true;
    }

    /// The motors the screen offers for `FRAME_CLASS` / `FRAME_TYPE` (the parameter values; the
    /// caller reads them). A frame without a table entry gets generic motors A, B, C...:
    /// `generic_count` (1 to 12, the user's choice) of them, or a default by vehicle type.
    pub fn frame(
        &self,
        class: Option<u32>,
        frame_type: Option<u32>,
        generic_count: Option<u8>,
    ) -> Result<MotorFrame, LinkError> {
        let (_, vehicle) = self.shared.link.selected_vehicle()?;
        let layout = class
            .zip(frame_type)
            .and_then(|(class, frame_type)| motor_layout(class, frame_type));
        Ok(match layout {
            Some(layout) => {
                let mut motors: Vec<MotorSlot> = layout
                    .motors
                    .iter()
                    .map(|m| MotorSlot {
                        label: label(m.test_order),
                        test_order: m.test_order,
                        output: Some(m.number),
                        rotation: (m.rotation != "?").then(|| m.rotation.clone()),
                    })
                    .collect();
                motors.sort_by_key(|m| m.test_order);
                MotorFrame {
                    known: true,
                    class_name: Some(layout.class_name.clone()),
                    type_name: Some(layout.type_name.clone()),
                    motors,
                }
            }
            None => MotorFrame {
                known: false,
                class_name: None,
                type_name: None,
                motors: (1..=generic_count
                    .map_or_else(|| fallback_motor_count(vehicle), |n| n.clamp(1, MAX_MOTORS)))
                    .map(|order| MotorSlot {
                        label: label(order),
                        test_order: order,
                        output: None,
                        rotation: None,
                    })
                    .collect(),
            },
        })
    }

    /// Asks the vehicle to stream the ESC telemetry messages while the screen is open, and puts
    /// them back on the vehicle's own rate when it closes.
    pub fn watch_esc(&self, enabled: bool) -> Result<(), LinkError> {
        let (id, _) = self.shared.link.selected_vehicle()?;
        let interval = if enabled { ESC_STREAM_INTERVAL_US } else { 0 };
        for message in ESC_MESSAGES {
            self.shared
                .link
                .send(&commands::set_message_interval(id, message, interval))?;
        }
        Ok(())
    }

    /// Refuses to start unless the vehicle is there, disarmed and not already testing.
    fn begin(
        &self,
        request: &MotorTestRequest,
    ) -> Result<control_station_vehicle_state::VehicleId, LinkError> {
        validate(request)?;
        let (id, _) = self.shared.link.selected_vehicle()?;
        if self.shared.link.telemetry().armed {
            return Err(error(
                "the vehicle is armed; disarm it before testing motors",
            ));
        }
        if self.lock().running() {
            return Err(error("a motor test is already running; press Stop first"));
        }
        Ok(id)
    }

    fn throttle_of(request: &MotorTestRequest) -> f32 {
        if request.throttle_type == MotorThrottleType::Pilot {
            0.0
        } else {
            request.throttle
        }
    }

    fn started(&self, seconds: f32, message: String) -> String {
        self.update(|inner| {
            inner.busy_until = Some(Instant::now() + Duration::from_secs_f32(seconds));
            inner.message = message.clone();
        });
        message
    }

    /// Spins one motor.
    pub async fn start(&self, request: MotorTestRequest) -> Result<String, LinkError> {
        if request.motor == 0 || request.motor > MAX_MOTORS {
            return Err(error("pick a motor to test"));
        }
        let id = self.begin(&request)?;
        self.shared
            .link
            .send_command(commands::motor_test(
                id,
                request.motor,
                request.throttle_type,
                Self::throttle_of(&request),
                request.seconds,
                0,
            ))
            .await?;
        Ok(self.started(
            request.seconds,
            format!(
                "Motor {} spinning for {} s",
                label(request.motor),
                request.seconds
            ),
        ))
    }

    /// Spins every motor at once: one command per motor, back to back, as the legacy tool does.
    pub async fn start_all(
        &self,
        request: MotorTestRequest,
        count: u8,
    ) -> Result<String, LinkError> {
        let count = check_count(count)?;
        let id = self.begin(&request)?;
        for motor in 1..=count {
            self.shared
                .link
                .send_command(commands::motor_test(
                    id,
                    motor,
                    request.throttle_type,
                    Self::throttle_of(&request),
                    request.seconds,
                    0,
                ))
                .await?;
        }
        Ok(self.started(
            request.seconds,
            format!("All {count} motors spinning for {} s", request.seconds),
        ))
    }

    /// Runs the motors one after the other; the vehicle runs the sequence itself.
    pub async fn start_sequence(
        &self,
        request: MotorTestRequest,
        count: u8,
    ) -> Result<String, LinkError> {
        let count = check_count(count)?;
        let id = self.begin(&request)?;
        self.shared
            .link
            .send_command(commands::motor_test(
                id,
                1,
                request.throttle_type,
                Self::throttle_of(&request),
                request.seconds,
                count,
            ))
            .await?;
        Ok(self.started(
            request.seconds * f32::from(count),
            format!(
                "Testing {count} motors in sequence, {} s each",
                request.seconds
            ),
        ))
    }

    /// Stops every motor. Allowed at any time (armed, busy or idle); every motor is asked even
    /// if one of them fails, and the first failure is returned.
    pub async fn stop_all(&self, count: u8) -> Result<String, LinkError> {
        let count = count.clamp(1, MAX_MOTORS);
        let (id, _) = self.shared.link.selected_vehicle()?;
        let mut first_error = None;
        for motor in 1..=count {
            let stop = commands::motor_test(id, motor, MotorThrottleType::Percent, 0.0, 0.0, 0);
            if let Err(e) = self.shared.link.send_command(stop).await {
                first_error.get_or_insert(e);
            }
        }
        self.update(|inner| {
            inner.busy_until = None;
            inner.message = "Motors stopped".into();
        });
        match first_error {
            Some(e) => Err(e),
            None => Ok("Motors stopped".into()),
        }
    }

    /// Writes `MOT_SPIN_ARM` or `MOT_SPIN_MIN` (the screen shows percent, the parameter is 0..1).
    pub async fn set_spin(&self, which: SpinParam, percent: f32) -> Result<String, LinkError> {
        if !percent.is_finite() || !(0.0..=MAX_SPIN_PERCENT).contains(&percent) {
            return Err(error(format!(
                "the spin value must be between 0 and {MAX_SPIN_PERCENT:.0} %"
            )));
        }
        self.shared.link.selected_vehicle()?;
        if self.shared.link.telemetry().armed {
            return Err(error("the vehicle is armed; disarm it first"));
        }
        let value = f64::from(percent) / 100.0;
        (self.shared.params)(which.name().into(), value)
            .await
            .map_err(error)?;
        Ok(format!("{} set to {percent:.1} %", which.name()))
    }
}

fn check_count(count: u8) -> Result<u8, LinkError> {
    if count == 0 || count > MAX_MOTORS {
        Err(error(format!("the motor count must be 1 to {MAX_MOTORS}")))
    } else {
        Ok(count)
    }
}

/// Adds the four ESCs of one `ESC_TELEMETRY_*` message that report anything.
fn collect(
    out: &mut Vec<EscReading>,
    first: u8,
    rpm: &[u16; 4],
    voltage: &[u16; 4],
    current: &[u16; 4],
    temperature: &[u8; 4],
) {
    for i in 0..4 {
        if rpm[i] == 0 && voltage[i] == 0 && current[i] == 0 && temperature[i] == 0 {
            continue;
        }
        out.push(EscReading {
            index: first + i as u8,
            rpm: Some(i32::from(rpm[i])),
            voltage: Some(f32::from(voltage[i]) / 100.0),
            current: Some(f32::from(current[i]) / 100.0),
            temperature: Some(temperature[i]),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: MotorThrottleType, throttle: f32, seconds: f32) -> MotorTestRequest {
        MotorTestRequest {
            motor: 1,
            throttle_type: kind,
            throttle,
            seconds,
            allow_high: false,
        }
    }

    #[test]
    fn low_percent_throttle_is_accepted() {
        assert!(validate(&request(MotorThrottleType::Percent, 5.0, 2.0)).is_ok());
        assert!(validate(&request(MotorThrottleType::Percent, 20.0, 2.0)).is_ok());
    }

    #[test]
    fn high_throttle_needs_confirmation() {
        let mut high = request(MotorThrottleType::Percent, 40.0, 2.0);
        assert!(validate(&high).is_err());
        high.allow_high = true;
        assert!(validate(&high).is_ok());
        high.throttle = 101.0;
        assert!(validate(&high).is_err());
    }

    #[test]
    fn bad_durations_and_throttles_are_refused() {
        assert!(validate(&request(MotorThrottleType::Percent, 5.0, 0.0)).is_err());
        assert!(validate(&request(MotorThrottleType::Percent, 5.0, 500.0)).is_err());
        assert!(validate(&request(MotorThrottleType::Percent, 5.0, f32::NAN)).is_err());
        assert!(validate(&request(MotorThrottleType::Percent, -1.0, 2.0)).is_err());
        assert!(validate(&request(MotorThrottleType::Pwm, 500.0, 2.0)).is_err());
        assert!(validate(&request(MotorThrottleType::Pwm, 1100.0, 2.0)).is_ok());
        assert!(validate(&request(MotorThrottleType::Pilot, 0.0, 2.0)).is_ok());
    }

    #[test]
    fn labels_follow_the_alphabet() {
        assert_eq!(label(1), "A");
        assert_eq!(label(4), "D");
        assert_eq!(label(12), "L");
    }
}
