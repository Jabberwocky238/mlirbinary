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

`SpirvFormat` (feature `spirv`, default) reads and writes SPIR-V as `mlir-translate` (LLVM 23.1.5) does,
checked against MLIR's own 250 `Target/SPIRV` test modules, all of them:

- reading (`--deserialize-spirv`, `cargo test --test read`): structured control flow rebuilt from
  the merge instructions, OpPhi as block arguments, constants materialized at their uses,
  identified and recursive structs, spec constants and their operations, ARM tensors and graphs;
  the text compared is the generic form with SSA names renumbered. `SpirvReader { structurize:
  false }` is `--spirv-structurize-control-flow=false`: blocks stay a plain graph, which also reads
  what structurization cannot, such as a loop left early (`tests/fixtures/spirv-unstructured`);
- writing (`--serialize-spirv`, `cargo test --test write`): byte for byte;
- reading then writing (`cargo test --test roundtrip`): byte for byte what `mlir-translate`
  writes for the module it reads.

The grammar tables (`src/spirv/tables.gen.rs`) are generated from the SPIR-V dialect's ODS by
`scripts/spirv_tables.py`.

`Ir` is the read-only index the formats work on: every op, value, block and region of an mlirformat
op tree numbered, operands and successors resolved by name.

## Features

| feature | default | what it is |
|---|---|---|
| `std` | yes | the standard library (without it the crate is `no_std` and needs `alloc`), mlirformat's `std` |
| `spirv` | yes | the SPIR-V binary format |

## License

MIT
