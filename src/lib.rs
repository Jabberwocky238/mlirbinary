#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod format;
mod ir;
mod tree;
mod types;

pub use format::{Binaries, Pool, Read, Write};
pub use ir::{BlockId, Ir, OpId, RegionId, ValueId};
pub use types::*;
