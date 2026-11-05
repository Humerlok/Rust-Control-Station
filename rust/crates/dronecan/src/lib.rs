//! DroneCAN (UAVCAN v0) over CAN: the DSDL catalog, the bit-level codec and the CAN transfer
//! layer. Pure: no I/O, no async. The vendored DSDL under `dsdl/` is embedded at build time.
pub mod codec;
pub mod dsdl;
pub mod image;
pub mod transfer;

pub use codec::{CodecError, Side, Value};
pub use dsdl::{Catalog, DsdlError};
