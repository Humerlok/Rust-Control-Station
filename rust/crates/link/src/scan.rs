use std::time::Duration;

use control_station_mavlink::Parser;
use control_station_transport::{list_serial_ports, open, Connection, Endpoint};

use crate::types::LinkError;

const BAUDS: [u32; 3] = [115_200, 57_600, 921_600];
const PROBE_TIME: Duration = Duration::from_millis(1500);
const SCAN_TIMEOUT: Duration = Duration::from_secs(50);

/// Tries each serial port at the common bauds and returns the first that speaks MAVLink.
pub async fn scan_serial() -> Result<Connection, LinkError> {
    let scan = async {
        let ports = list_serial_ports();
        if ports.is_empty() {
            return Err(LinkError::Other("no serial ports found".into()));
        }
        for port in &ports {
            for baud in BAUDS {
                let endpoint = Endpoint::Serial {
                    port: port.name.clone(),
                    baud,
                };
                let Ok(mut connection) = open(&endpoint).await else {
                    break;
                };
                if hears_mavlink(&mut connection).await {
                    return Ok(connection);
                }
            }
        }
        Err(LinkError::Other(
            "no MAVLink device found on any serial port".into(),
        ))
    };
    tokio::time::timeout(SCAN_TIMEOUT, scan)
        .await
        .map_err(|_| LinkError::Other("serial scan timed out".into()))?
}

async fn hears_mavlink(connection: &mut Connection) -> bool {
    let listen = async {
        let mut parser = Parser::new();
        let mut buf = [0u8; 1024];
        loop {
            match connection.reader.read(&mut buf).await {
                Ok(n) => {
                    if !parser.push(&buf[..n]).is_empty() {
                        return true;
                    }
                }
                Err(_) => return false,
            }
        }
    };
    tokio::time::timeout(PROBE_TIME, listen)
        .await
        .unwrap_or(false)
}
