# mlirbinary

[English](README.md)

用 Rust 读写 MLIR 模块的二进制格式：把 [mlirformat](https://github.com/Jabberwocky238/mlirformat) 的 op 树写成字节，再读回来。每种格式是一个插件：实现 `Format` 的类型，装进 `Binaries`，跟方言装进 mlirformat 的 `MlirOpt` 是同一个写法；别的 crate 不改 mlirbinary 就能接入自己的格式（比如某个 GPU 的机器码）。

```rust
use mlirbinary::{Binaries, Format};

let mut binaries = Binaries::builtin();       // 本 crate 自带的格式（SPIR-V）
binaries.load(MyGpuFormat);                   // 再加一个别的 crate 的
let bytes = binaries.write("spirv", &module)?; // `module`：mlirformat 的 `spirv.module` op
let op = binaries.read(&bytes)?;              // 认得这串字节的格式负责读
```

```rust
pub trait Format: Send + Sync {
    fn name(&self) -> &'static str;
    fn recognizes(&self, bytes: &[u8]) -> bool;
    fn write(&self, op: &Operation<'_>) -> Result<Vec<u8>, Error>;
    fn read(&self, bytes: &[u8]) -> Result<Operation<'static>, Error>;
}
```

没有全局状态：一个程序有哪些格式，就是它自己装了哪些。

## SPIR-V

`SpirvFormat`（feature `spirv`，缺省开）按 `mlir-translate`（LLVM 23.1.5）的方式读写 SPIR-V，用 MLIR 自己的 250 个 `Target/SPIRV` 测试模块对拍，全部通过：

- 读入（`--deserialize-spirv`，`cargo test --test read`）：按 merge 指令还原结构化控制流、OpPhi 变块参数、常量在使用处物化、具名与递归结构体、特化常量及其运算、ARM 张量与 graph；比较的是通用形式文本，SSA 名重新编号。`SpirvReader { structurize: false }` 即 `--spirv-structurize-control-flow=false`：块保持平铺的分支图，结构化读不了的（比如循环里提前退出）也能读（`tests/fixtures/spirv-unstructured`）；
- 写入（`--serialize-spirv`，`cargo test --test write`）：逐字节一致；
- 读入再写出（`cargo test --test roundtrip`）：与 `mlir-translate` 读入同一模块后写出的结果逐字节一致。

指令表 `src/spirv/tables.gen.rs` 由 `scripts/spirv_tables.py` 从 SPIR-V 方言的 ODS 生成。

`Ir` 是各格式共用的只读索引：mlirformat op 树里每个 op、值、块、区域编号，操作数和后继按名字解析好。

## Features

| feature | 缺省 | 是什么 |
|---|---|---|
| `std` | 是 | 标准库（不开时 crate 是 `no_std`，需要 `alloc`），以及 mlirformat 的 `std` |
| `spirv` | 是 | SPIR-V 二进制格式 |

## 许可

MIT
