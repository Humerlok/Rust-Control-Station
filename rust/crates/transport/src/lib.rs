//! Transport contracts; concrete serial support is feature-gated until platform integration is added.
use std::{future::Future, pin::Pin};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Serial { port: String, baud_rate: u32 },
    Udp { bind: String },
    Tcp { address: String },
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("connection closed")]
    Closed,
}

pub trait Link: Send + Sync {
    fn read<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'a>>;
    fn write<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}
impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            initial_delay_ms: 500,
            max_delay_ms: 10_000,
        }
    }
}
