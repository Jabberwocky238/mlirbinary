use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use mlirformat::{Error, Operation};
use moremath::{Executor, ThreadSingle};

/// The thread pool a format runs its work on: any moremath `Executor` (`ThreadSingle`,
/// `ThreadMulti`), the same pools mlirformat's `parse_in` / `print_in` take, seen through an
/// interface a `dyn Format` can take
pub trait Pool: Sync {
    /// How many tasks it may run at once
    fn threads(&self) -> usize;

    /// Runs `tasks`, returning once all are done
    fn run<'t>(&self, tasks: Vec<Box<dyn FnOnce() + Send + 't>>);
}

impl<X: Executor + Sync> Pool for X {
    fn threads(&self) -> usize {
        Executor::threads(self)
    }

    fn run<'t>(&self, tasks: Vec<Box<dyn FnOnce() + Send + 't>>) {
        self.execute(tasks);
    }
}

/// A binary format: how one kind of op (a `spirv.module`, a GPU's kernel) is written as bytes, and
/// read back. A crate outside mlirbinary implements it for its own format and hands it to
/// `Binaries::load`, the way a dialect goes to mlirformat's `MlirOpt::load`.
pub trait Format: Send + Sync {
    /// The name `Binaries::write` and `Binaries::get` take ("spirv")
    fn name(&self) -> &'static str;

    /// Whether `bytes` are in this format (its magic number); `Binaries::read` asks each format
    fn recognizes(&self, bytes: &[u8]) -> bool;

    /// `op` (the format's root op) as bytes, work spread over `pool`
    fn write(&self, op: &Operation<'_>, pool: &dyn Pool) -> Result<Vec<u8>, Error>;

    /// The op `bytes` hold, in generic form, work spread over `pool`
    fn read(&self, bytes: &[u8], pool: &dyn Pool) -> Result<Operation<'static>, Error>;
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

    /// Every format this crate has
    pub fn builtin() -> Self {
        let mut b = Self::new();
        b.load(crate::SpirvFormat::default());
        b.load(crate::BytecodeFormat::default());
        b.load(crate::GlslFormat::default());
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

    /// `op` in format `name`, on this thread
    pub fn write(&self, name: &str, op: &Operation<'_>) -> Result<Vec<u8>, Error> {
        self.write_in(name, op, &ThreadSingle)
    }

    /// `op` in format `name`, on the threads of `x`
    pub fn write_in<X: Executor + Sync>(&self, name: &str, op: &Operation<'_>, x: &X) -> Result<Vec<u8>, Error> {
        self.get(name).ok_or_else(|| Error(format!("no binary format {name}")))?.write(op, x)
    }

    /// Reads `bytes` with the first format that recognizes them, on this thread
    pub fn read(&self, bytes: &[u8]) -> Result<Operation<'static>, Error> {
        self.read_in(bytes, &ThreadSingle)
    }

    /// Reads `bytes` with the first format that recognizes them, on the threads of `x`
    pub fn read_in<X: Executor + Sync>(&self, bytes: &[u8], x: &X) -> Result<Operation<'static>, Error> {
        self.formats.iter().find(|f| f.recognizes(bytes)).ok_or_else(|| Error(format!("no binary format recognizes these {} bytes", bytes.len())))?.read(bytes, x)
    }
}
