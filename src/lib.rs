#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod format;
mod ir;
#[cfg(feature = "spirv")]
mod spirv;

pub use format::{Binaries, Format};
pub use ir::{BlockId, Ir, OpId, RegionId, ValueId};
#[cfg(feature = "spirv")]
pub use spirv::*;
