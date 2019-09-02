//! Input/output of Bifrost-related data.

pub mod utils;
pub mod snapshot;

/// Little- or big-endian byte order.
#[derive(Debug, Copy, Clone)]
pub enum Endianness {
    Little,
    Big
}