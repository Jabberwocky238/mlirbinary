mod emit;
mod entity;
#[path = "props.gen.rs"]
mod props;
mod quant;
mod read;
mod write;

use alloc::string::String;
use alloc::vec::Vec;

use mlirformat::{Error, OpInfos, Operation};

use crate::{Format, Pool};

/// How a property of an op is written, as its `writeProperties` does
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropKind {
    /// An attribute that is always there
    Attr,
    /// An attribute that may be absent
    OptAttr,
    /// `operandSegmentSizes` / `resultSegmentSizes`, as a sparse array, of this many segments
    Segments(u8),
    /// An integer or enum stored natively, shown as an integer attribute this wide
    VarInt(u8),
    SignedVarInt(u8),
    Bool,
    /// An optional integer: whether it is there, then it
    OptVarInt(u8),
    /// A list of integers
    VarInts(u8),
    String,
}

/// A property of an op, by name
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Prop {
    pub name: &'static str,
    pub kind: PropKind,
}

/// What an op's encoding says it has (`bytecode::OpEncodingMask`)
struct Mask;

impl Mask {
    const ATTRS: u8 = 0b0000_0001;
    const RESULTS: u8 = 0b0000_0010;
    const OPERANDS: u8 = 0b0000_0100;
    const SUCCESSORS: u8 = 0b0000_1000;
    const INLINE_REGIONS: u8 = 0b0001_0000;
    const USE_LIST_ORDERS: u8 = 0b0010_0000;
    const PROPERTIES: u8 = 0b0100_0000;
}

/// The magic number a bytecode file starts with: "MLïR"
const MAGIC: &[u8] = b"ML\xefR";
/// The bytecode version written (`bytecode::kVersion`)
const VERSION: u64 = 6;

/// MLIR's own binary form of a module, the bytecode `mlir-opt --emit-bytecode` writes
pub struct BytecodeFormat {
    infos: OpInfos,
    /// Who wrote it, in the file's header
    pub producer: String,
}

impl BytecodeFormat {
    /// The producer `mlir-opt` 23.1.5 names
    pub const UPSTREAM_PRODUCER: &'static str = "MLIR23.1.5";

    /// The ops MLIR itself defines are registered: their properties are written natively
    pub fn new(infos: OpInfos) -> Self {
        Self { infos, producer: String::from(Self::UPSTREAM_PRODUCER) }
    }
}

impl Default for BytecodeFormat {
    fn default() -> Self {
        Self::new(OpInfos::upstream())
    }
}

impl Format for BytecodeFormat {
    fn name(&self) -> &'static str {
        "mlirbc"
    }

    fn recognizes(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(MAGIC)
    }

    fn write(&self, op: &Operation<'_>, _pool: &dyn Pool) -> Result<Vec<u8>, Error> {
        write::Writer::new(op, &self.infos)?.write(&self.producer)
    }

    fn read(&self, bytes: &[u8], _pool: &dyn Pool) -> Result<Operation<'static>, Error> {
        read::Reader::new(bytes)?.read()
    }
}
