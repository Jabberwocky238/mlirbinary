# mlirbinary

[中文](README.zh-CN.md)

Binary formats of MLIR modules in Rust: an op tree of [mlirformat](https://github.com/Jabberwocky238/mlirformat)
written as bytes and read back. Each format is a plugin: a type implementing `Format`, loaded into a
`Binaries` the way a dialect is loaded into mlirformat's `MlirOpt`; a crate outside this one adds its
own format (a GPU's machine code, say) without changing mlirbinary.

```rust
use mlirbinary::{Binaries, Format};

let mut binaries = Binaries::builtin();       // the formats this crate has (SPIR-V)
binaries.load(MyGpuFormat);                   // and one from another crate
let bytes = binaries.write("spirv", &module)?; // `module`: an mlirformat `spirv.module` op
let op = binaries.read(&bytes)?;              // the format that recognizes the bytes reads them
```

```rust
pub trait Format: Send + Sync {
    fn name(&self) -> &'static str;
    fn recognizes(&self, bytes: &[u8]) -> bool;
    fn write(&self, op: &Operation<'_>) -> Result<Vec<u8>, Error>;
    fn read(&self, bytes: &[u8]) -> Result<Operation<'static>, Error>;
}
```

Nothing is global: a program's formats are the ones it loads.

## SPIR-V

`SpirvFormat` (feature `spirv`, default) writes a `spirv.module` as `mlir-translate --serialize-spirv`
(LLVM 23.1.5) does: 196 of MLIR's 250 `Target/SPIRV` test modules come out byte for byte
(`cargo test --test write`; the others use ops and decorations not written yet). Reading is in progress.

`Ir` is the read-only index the formats work on: every op, value, block and region of an mlirformat
op tree numbered, operands and successors resolved by name.

## Features

| feature | default | what it is |
|---|---|---|
| `std` | yes | the standard library (without it the crate is `no_std` and needs `alloc`), mlirformat's `std` |
| `spirv` | yes | the SPIR-V binary format |

## License

MIT
