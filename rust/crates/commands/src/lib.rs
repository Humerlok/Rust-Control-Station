use control_station_vehicle_state::VehicleId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParameterValue {
    Int(i32),
    Float(f32),
    Text(String),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub value: ParameterValue,
    pub component_id: u8,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetParameter {
    pub target: VehicleId,
    pub parameter: Parameter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalibrationKind {
    Accelerometer,
    Compass,
    Gyroscope,
    Level,
    Pressure,
    Radio,
    Esc,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalibrationStatus {
    Ready,
    Running,
    Succeeded,
    Cancelled,
    Failed(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationSession {
    pub target: VehicleId,
    pub kind: CalibrationKind,
    pub status: CalibrationStatus,
    pub instruction: String,
}

impl CalibrationSession {
    pub fn begin(target: VehicleId, kind: CalibrationKind) -> Self {
        Self {
            target,
            kind,
            status: CalibrationStatus::Running,
            instruction: "Follow the vehicle prompts and keep the telemetry link connected.".into(),
        }
    }
    pub fn complete(&mut self, success: bool, message: impl Into<String>) {
        self.status = if success {
            CalibrationStatus::Succeeded
        } else {
            CalibrationStatus::Failed(message.into())
        };
    }
    pub fn cancel(&mut self) {
        self.status = CalibrationStatus::Cancelled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calibration_can_be_cancelled() {
        let mut session = CalibrationSession::begin(
            VehicleId {
                system_id: 1,
                component_id: 1,
            },
            CalibrationKind::Compass,
        );
        session.cancel();
        assert_eq!(session.status, CalibrationStatus::Cancelled);
    }
}
