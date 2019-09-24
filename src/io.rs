//! File input/output.

pub mod snapshot;
pub mod utils;

/// Little- or big-endian byte order.
#[derive(Clone, Copy, Debug)]
pub enum Endianness {
    Little,
    Big,
}

/// Whether or not to print non-critical status messages.
#[derive(Clone, Copy, Debug)]
pub enum Verbose {
    Yes,
    No,
}

impl Verbose {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn is_yes(&self) -> bool {
        match *self {
            Verbose::Yes => true,
            Verbose::No => false,
        }
    }
}
