#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "bytecode")]
mod bytecode;
mod format;
mod ir;
#[cfg(feature = "spirv")]
mod spirv;

#[cfg(feature = "bytecode")]
pub use bytecode::{BytecodeFormat, Prop, PropKind};
pub use format::{Binaries, Format};
pub use ir::{BlockId, Ir, OpId, RegionId, ValueId};
#[cfg(feature = "spirv")]
pub use spirv::*;
