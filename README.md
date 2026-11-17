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

- reading (`--deserialize-spirv`, `cargo test --test spirv read`): structured control flow rebuilt from
  the merge instructions, OpPhi as block arguments, constants materialized at their uses,
  identified and recursive structs, spec constants and their operations, ARM tensors and graphs;
  the text compared is the generic form with SSA names renumbered. `SpirvReader { structurize:
  false }` is `--spirv-structurize-control-flow=false`: blocks stay a plain graph, which also reads
  what structurization cannot, such as a loop left early (`tests/spirv/fixtures/unstructured`);
- writing (`--serialize-spirv`, `cargo test --test spirv write`): byte for byte;
- reading then writing (`cargo test --test spirv roundtrip`): byte for byte what `mlir-translate`
  writes for the module it reads.

`SpirvGlsl` (`"spirv-glsl"`, `Write`) writes a `spirv.module` as GLSL 4.50 source for Vulkan, as far as
GLSL can say what it does (it says why where it cannot); `SpirvGlsl::stage` is the entry point's stage.
Its output for each test module is kept beside it (`tests/spirv/fixtures/modules/*.glsl`, `cargo test --test
spirv glsl`); mlirpass compiles what it writes with glslang and runs it on Vulkan.

The grammar tables (`src/types/spirv/tables.gen.rs`) are generated from the SPIR-V dialect's ODS by
`scripts/spirv_tables.py`.

## MLIR bytecode

`BytecodeFormat` (`"mlirbc"`, both `Read` and `Write`) reads and writes MLIR's own binary form,
the bytecode `mlir-opt --emit-bytecode` (23.1.5) writes, version 6. Checked against `mlir-opt` on
one module of each upstream test file (2269, `tests/bytecode/fixtures`, made by
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
`scripts/mlirbc_props.py` reads it off an LLVM build into `src/types/bytecode/props.gen.rs`, which also
lists the ops `mlir-opt` registers. The test dialect, which only `mlir-opt` has, is left out.

## GLSL

`GlslFormat` (`"glsl"`, both `Read` and `Write`) reads GLSL ES 1.00 / 3.00 source into mlirformat's `glsl` dialect: `glsl.shader`,
its globals, built-ins and texture ops, the rest as `func` / `arith` / `math` / `vector` / `scf` and SPIR-V
control flow and types — what mlirpass's `convert-glsl-to-spirv` lowers. `GlslFormat::new(Some(stage))`
names the stage; without one, a source that writes `gl_Position` is a vertex shader. Checked on mlirpass's
101 test shaders (`tests/glsl/fixtures`, `cargo test --test glsl`): each reads as mlirpass read it before
the reader moved here.

Writing turns a `glsl.shader` back into source: each value a local, loops and branches rebuilt from
the structure reading left (selection merges, the loop layout), `int` / `uint` conversions where a
value's signedness is only in its op, explicit locations, bindings, interpolation and precision
kept. Struct members and uniform blocks get names (`m0`, `S_<hash>`, `<instance>_Block`): the dialect
keeps none. Every test shader written reads back (`cargo test --test glsl`); in mlirpass's
`tests/gl.rs` each one read and written draws on the local OpenGL bit for bit as its source does
(where Mesa reassociates the source, as glslang's SPIR-V does).

## GPU machine code and images

Each GPU's instruction-set dialect (mlirformat's `apple9`, `agx`, `rdna2`) encoded as the hardware runs it,
and an image of command streams and records written as its memories' bytes:

- `Apple9Isa`, `Apple9Code`, `Apple9Writer` (`"apple9"`): Apple G13 instructions, jumps resolved against
  their labels; checked against what an M4 ran (`cargo test --test apple9`).
- `AgxIsa`, `AgxCode`, `AgxRecord`, `AgxImage`, `AgxWriter` (`"agx"`): USC instructions by their encoding
  tables, command-buffer structs by their layouts; checked against Mesa's AGX2 disassembler cases and
  Mesa's `agx_pack` (`cargo test --test agx`, oracles `scripts/agx_*_oracle.*`).
- `Rdna2Isa`, `Rdna2Words`, `Rdna2Pm4`, `Rdna2Code`, `Rdna2Image`, `Rdna2Writer` (`"rdna2"`): GFX10
  instructions and PM4 packets; checked against `llvm-mc -mcpu=gfx1036` and Mesa's `S_*` register macros
  (`cargo test --test rdna2`, oracles `scripts/rdna2_*_oracle.sh`).
- `Pack`, `Template`: a record of a dialect's layout tables packed into dwords (a `Template` keeps the
  fields still waiting for an address). `ImageWriter` writes an `image.memory` container laid out by
  mlirpass, each GPU giving its streams' and records' bytes through `ImageTarget`.

The writers write every kernel's code in an op, one after the other; `<Gpu>Code::of` gives one kernel's
code with what launching it needs. Lowering to these dialects is mlirpass's.

The crate root holds what formats and their users share: the `Read` / `Write` traits, `Pool`,
`Binaries`, and the trees (`Ir`, read-only, which writers walk; a mutable one readers build in). Each
format lives in `src/types/<format>/`.

`Ir` is the read-only index the formats work on: every op, value, block and region of an mlirformat
op tree numbered, operands and successors resolved by name.

## Features

| feature | default | what it is |
|---|---|---|
| `std` | yes | the standard library (without it the crate is `no_std` and needs `alloc`), mlirformat's `std` |

## License

MIT
