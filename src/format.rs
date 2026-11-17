use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use mlirformat::{Error, Operation};

/// A binary format: how one kind of op (a `spirv.module`, a GPU's kernel) is written as bytes, and
/// read back. A crate outside mlirbinary implements it for its own format and hands it to
/// `Binaries::load`, the way a dialect goes to mlirformat's `MlirOpt::load`.
pub trait Format: Send + Sync {
    /// The name `Binaries::write` and `Binaries::get` take ("spirv")
    fn name(&self) -> &'static str;

    /// Whether `bytes` are in this format (its magic number); `Binaries::read` asks each format
    fn recognizes(&self, bytes: &[u8]) -> bool;

    /// `op` (the format's root op) as bytes
    fn write(&self, op: &Operation<'_>) -> Result<Vec<u8>, Error>;

    /// The op `bytes` hold, in generic form
    fn read(&self, bytes: &[u8]) -> Result<Operation<'static>, Error>;
}

/// The formats a program knows, loaded one by one; nothing is global.
#[derive(Default)]
pub struct Binaries {
    formats: Vec<Box<dyn Format>>,
}

impl Binaries {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every format this crate has (with its features)
    #[cfg_attr(not(any(feature = "spirv", feature = "bytecode")), allow(unused_mut))]
    pub fn builtin() -> Self {
        let mut b = Self::new();
        #[cfg(feature = "spirv")]
        b.load(crate::SpirvFormat::default());
        #[cfg(feature = "bytecode")]
        b.load(crate::BytecodeFormat::default());
        b
    }

    /// Adds `format`, replacing one of the same name
    pub fn load(&mut self, format: impl Format + 'static) -> &mut Self {
        self.formats.retain(|f| f.name() != format.name());
        self.formats.push(Box::new(format));
        self
    }

    pub fn get(&self, name: &str) -> Option<&dyn Format> {
        self.formats.iter().find(|f| f.name() == name).map(|f| &**f)
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.formats.iter().map(|f| f.name()).collect()
    }

    pub fn write(&self, name: &str, op: &Operation<'_>) -> Result<Vec<u8>, Error> {
        self.get(name).ok_or_else(|| Error(format!("no binary format {name}")))?.write(op)
    }

    /// Reads `bytes` with the first format that recognizes them
    pub fn read(&self, bytes: &[u8]) -> Result<Operation<'static>, Error> {
        self.formats.iter().find(|f| f.recognizes(bytes)).ok_or_else(|| Error(format!("no binary format recognizes these {} bytes", bytes.len())))?.read(bytes)
    }
}
