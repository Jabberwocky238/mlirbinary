#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod bytecode;
mod format;
mod ir;
mod spirv;

pub use bytecode::{BytecodeFormat, Prop, PropKind};
pub use format::{Binaries, Format};
pub use ir::{BlockId, Ir, OpId, RegionId, ValueId};
pub use spirv::*;
