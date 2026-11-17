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

/// Reading a format: the op some bytes hold. A crate outside mlirbinary implements it for its own
/// format and hands it to `Binaries::reads`, the way a dialect goes to mlirformat's `MlirOpt::load`.
pub trait Read: Send + Sync {
    /// The format's name ("spirv")
    fn name(&self) -> &'static str;

    /// Whether `bytes` are in this format (its magic number); `Binaries::read` asks each reader
    fn recognizes(&self, bytes: &[u8]) -> bool;

    /// The op `bytes` hold, in generic form, work spread over `pool`
    fn read(&self, bytes: &[u8], pool: &dyn Pool) -> Result<Operation<'static>, Error>;
}

/// Writing a format: an op (the format's root op, a `spirv.module`, a GPU's kernel) as bytes
pub trait Write: Send + Sync {
    /// The format's name, the one `Binaries::write` takes ("spirv")
    fn name(&self) -> &'static str;

    /// `op` as bytes, work spread over `pool`
    fn write(&self, op: &Operation<'_>, pool: &dyn Pool) -> Result<Vec<u8>, Error>;
}

/// The formats a program reads and writes, loaded one by one; nothing is global.
#[derive(Default)]
pub struct Binaries {
    readers: Vec<Box<dyn Read>>,
    writers: Vec<Box<dyn Write>>,
}

impl Binaries {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every format this crate has
    pub fn builtin() -> Self {
        let mut b = Self::new();
        b.reads(crate::SpirvReader::default()).writes(crate::SpirvWriter::default());
        b.reads(crate::BytecodeFormat::default()).writes(crate::BytecodeFormat::default());
        b.reads(crate::GlslFormat::default()).writes(crate::GlslFormat::default());
        b
    }

    /// Adds `reader`, replacing one of the same name
    pub fn reads(&mut self, reader: impl Read + 'static) -> &mut Self {
        self.readers.retain(|f| f.name() != reader.name());
        self.readers.push(Box::new(reader));
        self
    }

    /// Adds `writer`, replacing one of the same name
    pub fn writes(&mut self, writer: impl Write + 'static) -> &mut Self {
        self.writers.retain(|f| f.name() != writer.name());
        self.writers.push(Box::new(writer));
        self
    }

    pub fn reader(&self, name: &str) -> Option<&dyn Read> {
        self.readers.iter().find(|f| f.name() == name).map(|f| &**f)
    }

    pub fn writer(&self, name: &str) -> Option<&dyn Write> {
        self.writers.iter().find(|f| f.name() == name).map(|f| &**f)
    }

    /// The formats read, then those written
    pub fn names(&self) -> (Vec<&'static str>, Vec<&'static str>) {
        (self.readers.iter().map(|f| f.name()).collect(), self.writers.iter().map(|f| f.name()).collect())
    }

    /// `op` in format `name`, on this thread
    pub fn write(&self, name: &str, op: &Operation<'_>) -> Result<Vec<u8>, Error> {
        self.write_in(name, op, &ThreadSingle)
    }

    /// `op` in format `name`, on the threads of `x`
    pub fn write_in<X: Executor + Sync>(&self, name: &str, op: &Operation<'_>, x: &X) -> Result<Vec<u8>, Error> {
        self.writer(name).ok_or_else(|| Error(format!("no writer for format {name}")))?.write(op, x)
    }

    /// Reads `bytes` with the first reader that recognizes them, on this thread
    pub fn read(&self, bytes: &[u8]) -> Result<Operation<'static>, Error> {
        self.read_in(bytes, &ThreadSingle)
    }

    /// Reads `bytes` with the first reader that recognizes them, on the threads of `x`
    pub fn read_in<X: Executor + Sync>(&self, bytes: &[u8], x: &X) -> Result<Operation<'static>, Error> {
        self.readers.iter().find(|f| f.recognizes(bytes)).ok_or_else(|| Error(format!("no reader recognizes these {} bytes", bytes.len())))?.read(bytes, x)
    }
}
