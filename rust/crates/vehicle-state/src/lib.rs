use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VehicleId {
    pub system_id: u8,
    pub component_id: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Position {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude_m: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleSnapshot {
    pub vehicle: VehicleId,
    pub connected: bool,
    pub mode: Option<String>,
    pub armed: bool,
    pub position: Position,
    pub groundspeed_m_s: Option<f32>,
    pub battery_percent: Option<u8>,
    #[serde(skip)]
    pub last_heartbeat: Option<SystemTime>,
}

impl VehicleSnapshot {
    pub fn discovered(vehicle: VehicleId) -> Self {
        Self {
            vehicle,
            connected: true,
            mode: None,
            armed: false,
            position: Position::default(),
            groundspeed_m_s: None,
            battery_percent: None,
            last_heartbeat: Some(SystemTime::now()),
        }
    }
    pub fn heartbeat(&mut self) {
        self.connected = true;
        self.last_heartbeat = Some(SystemTime::now());
    }
    pub fn connection_lost(&mut self) {
        self.connected = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heartbeat_recovers_link() {
        let mut s = VehicleSnapshot::discovered(VehicleId {
            system_id: 1,
            component_id: 1,
        });
        s.connection_lost();
        s.heartbeat();
        assert!(s.connected);
    }
}
