//! The formats this crate reads and writes, one directory each; what they share (the `Read` /
//! `Write` traits, the pool, the trees) is at the crate's root.

mod bytecode;
mod glsl;
mod spirv;

pub use bytecode::{BytecodeFormat, Prop, PropKind};
pub use glsl::GlslFormat;
pub use spirv::*;
