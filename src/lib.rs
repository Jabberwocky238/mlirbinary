#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod bytecode;
mod format;
mod glsl;
mod ir;
mod tree;
mod spirv;

pub use bytecode::{BytecodeFormat, Prop, PropKind};
pub use format::{Binaries, Pool, Read, Write};
pub use glsl::GlslFormat;
pub use ir::{BlockId, Ir, OpId, RegionId, ValueId};
pub use spirv::*;
