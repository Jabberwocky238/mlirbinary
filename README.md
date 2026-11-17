# mlirbinary

[中文](README.zh-CN.md)

Binary (and source) formats of MLIR modules in Rust: an op tree of [mlirformat](https://github.com/Jabberwocky238/mlirformat)
written as bytes and read back. Each format is a plugin: a reader implementing `Read`, a writer implementing `Write` (a
format may have only one), loaded into a `Binaries` the way a dialect is loaded into mlirformat's
`MlirOpt`; a crate outside this one adds its own (a GPU's machine code, say) without changing mlirbinary.

```rust
use mlirbinary::Binaries;

let mut binaries = Binaries::builtin();       // the formats this crate reads and writes
binaries.reads(MyGpuReader).writes(MyGpuWriter); // and a reader and writer from another crate
let bytes = binaries.write("spirv", &module)?; // `module`: an mlirformat `spirv.module` op
let op = binaries.read(&bytes)?;              // the reader that recognizes the bytes reads them
```

```rust
pub trait Read: Send + Sync {
    fn name(&self) -> &'static str;
    fn recognizes(&self, bytes: &[u8]) -> bool;
    fn read(&self, bytes: &[u8], pool: &dyn Pool) -> Result<Operation<'static>, Error>;
}

pub trait Write: Send + Sync {
    fn name(&self) -> &'static str;
    fn write(&self, op: &Operation<'_>, pool: &dyn Pool) -> Result<Vec<u8>, Error>;
}
```

Nothing is global: a program's formats are the ones it loads.

Formats run on a thread pool: `Binaries::read_in` / `write_in` take any moremath `Executor`
(`ThreadSingle`, `ThreadMulti`), the pools mlirformat's `parse_in` / `print_in` take; `read` / `write`
run on the calling thread. A reader or writer sees the pool as `&dyn Pool`.

## SPIR-V

`SpirvReader` and `SpirvWriter` read and write SPIR-V as `mlir-translate` (LLVM 23.1.5) does,
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

## MLIR bytecode

`BytecodeFormat` (`"mlirbc"`, both `Read` and `Write`) reads and writes MLIR's own binary form,
the bytecode `mlir-opt --emit-bytecode` (23.1.5) writes, version 6. Checked against `mlir-opt` on
one module of each upstream test file (2269, `tests/fixtures/mlirbc`, made by
`scripts/mlirbc_fixtures.py`; `cargo test --test bytecode`):

- writing: 2243 byte for byte, numbered as `mlir-opt` numbers (attributes, types and op names by
  use count and dialect, values per region, isolated regions found from their uses, use-list
  orders from replaying the parser); builtin attributes and types and the quant dialect's types in
  their own encoding, other dialects' as text;
- reading: 2245 print in generic form as `mlir-opt` prints them, names included; 2243 written
  back to the same bytes;
- not yet: the llvm dialect's own encodings (debug info, loop annotations), floats under eight
  bits and `f8E8M0FNU`, dialect resources (`dense_resource` data).

How each op's properties are written comes from the code mlir-tblgen generates:
`scripts/mlirbc_props.py` reads it off an LLVM build into `src/bytecode/props.gen.rs`, which also
lists the ops `mlir-opt` registers. The test dialect, which only `mlir-opt` has, is left out.

## GLSL

`GlslFormat` (`"glsl"`, `Read` only so far) reads GLSL ES 1.00 / 3.00 source into mlirformat's `glsl` dialect: `glsl.shader`,
its globals, built-ins and texture ops, the rest as `func` / `arith` / `math` / `vector` / `scf` and SPIR-V
control flow and types — what mlirpass's `convert-glsl-to-spirv` lowers. `GlslFormat::new(Some(stage))`
names the stage; without one, a source that writes `gl_Position` is a vertex shader. Checked on mlirpass's
101 test shaders (`tests/fixtures/glsl`, `cargo test --test glsl`): each reads as mlirpass read it before
the reader moved here. Writing GLSL is not there yet.

`Ir` is the read-only index the formats work on: every op, value, block and region of an mlirformat
op tree numbered, operands and successors resolved by name.

## Features

| feature | default | what it is |
|---|---|---|
| `std` | yes | the standard library (without it the crate is `no_std` and needs `alloc`), mlirformat's `std` |

## License

MIT
