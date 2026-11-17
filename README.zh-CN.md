# mlirbinary

[English](README.md)

用 Rust 读写 MLIR 模块的二进制格式：把 [mlirformat](https://github.com/Jabberwocky238/mlirformat) 的 op 树写成字节，再读回来。每种格式是插件：读的一方实现 `Read`，写的一方实现 `Write`（一个格式可以只有其一），装进 `Binaries`，跟方言装进 mlirformat 的 `MlirOpt` 是同一个写法；别的 crate 不改 mlirbinary 就能接入自己的格式（比如某个 GPU 的机器码）。

```rust
use mlirbinary::Binaries;

let mut binaries = Binaries::builtin();       // 本 crate 能读写的格式
binaries.reads(MyGpuReader).writes(MyGpuWriter); // 再加别的 crate 的读、写
let bytes = binaries.write("spirv", &module)?; // `module`：mlirformat 的 `spirv.module` op
let op = binaries.read(&bytes)?;              // 认得这串字节的读者负责读
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

没有全局状态：一个程序有哪些格式，就是它自己装了哪些。

各格式在线程池上跑：`Binaries::read_in` / `write_in` 收任意 moremath `Executor`（`ThreadSingle`、`ThreadMulti`），和 mlirformat 的 `parse_in` / `print_in` 用的是同一种池；`read` / `write` 在调用线程上跑。读写双方看到的池是 `&dyn Pool`。

## SPIR-V

`SpirvReader` / `SpirvWriter` 按 `mlir-translate`（LLVM 23.1.5）的方式读写 SPIR-V，用 MLIR 自己的 250 个 `Target/SPIRV` 测试模块对拍，全部通过：

- 读入（`--deserialize-spirv`，`cargo test --test read`）：按 merge 指令还原结构化控制流、OpPhi 变块参数、常量在使用处物化、具名与递归结构体、特化常量及其运算、ARM 张量与 graph；比较的是通用形式文本，SSA 名重新编号。`SpirvReader { structurize: false }` 即 `--spirv-structurize-control-flow=false`：块保持平铺的分支图，结构化读不了的（比如循环里提前退出）也能读（`tests/fixtures/spirv-unstructured`）；
- 写入（`--serialize-spirv`，`cargo test --test write`）：逐字节一致；
- 读入再写出（`cargo test --test roundtrip`）：与 `mlir-translate` 读入同一模块后写出的结果逐字节一致。

`SpirvGlsl`（名字 `"spirv-glsl"`，实现 `Write`）把 `spirv.module` 写成面向 Vulkan 的 GLSL 4.50 源码，GLSL 表达不了的地方报明原因；`SpirvGlsl::stage` 给出入口点的阶段。每个测试模块的输出存在旁边（`tests/fixtures/spirv/*.glsl`，`cargo test --test spirv_glsl`）；mlirpass 用 glslang 编译它写出的源码，并在 Vulkan 上跑。

指令表 `src/types/spirv/tables.gen.rs` 由 `scripts/spirv_tables.py` 从 SPIR-V 方言的 ODS 生成。

## MLIR bytecode

`BytecodeFormat`（名字 `"mlirbc"`，`Read`、`Write` 都实现）读写 MLIR 自己的二进制形式，即 `mlir-opt --emit-bytecode`（23.1.5）写的 bytecode（第 6 版）。对拍取上游每个测试文件一个模块，共 2269 个（`tests/fixtures/mlirbc`，由 `scripts/mlirbc_fixtures.py` 生成；`cargo test --test bytecode`）：

- 写入：2243 个逐字节一致。编号与 `mlir-opt` 相同（属性、类型、op 名按引用次数和方言排、值按区域编号、按使用情况判定隔离区域、按解析器的建表顺序推出 use-list 顺序）；内置属性/类型和 quant 方言的类型用各自的专用编码，其他方言的写成文本；
- 读入：2245 个读出后打印成通用形式，与 `mlir-opt` 打印的完全一样（含值名）；2243 个读出再写回原样字节；
- 还没有：llvm 方言的专用编码（调试信息、循环标注）、八位以下的浮点和 `f8E8M0FNU`、方言资源（`dense_resource` 的数据）。

每个 op 的 properties 怎么写，取自 mlir-tblgen 生成的代码：`scripts/mlirbc_props.py` 从 LLVM 构建目录抽出来生成 `src/types/bytecode/props.gen.rs`，同时列出 `mlir-opt` 注册的全部 op。只有 `mlir-opt` 才有的 test 方言不在其中。

## GLSL

`GlslFormat`（名字 `"glsl"`，`Read`、`Write` 都实现）把 GLSL ES 1.00 / 3.00 源码读成 mlirformat 的 `glsl` 方言：`glsl.shader` 及其全局变量、内置函数和纹理 op，其余是 `func` / `arith` / `math` / `vector` / `scf` 和 SPIR-V 的控制流与类型，正是 mlirpass 的 `convert-glsl-to-spirv` 要降的东西。`GlslFormat::new(Some(stage))` 指定阶段；不指定时，写了 `gl_Position` 的算顶点着色器。用 mlirpass 的 101 个测试着色器对拍（`tests/fixtures/glsl`，`cargo test --test glsl`）：每个读出的结果都和读入搬来之前 mlirpass 读出的一样。

写出把 `glsl.shader` 还原成源码：每个值落成局部变量，循环和分支按读入留下的结构（选择的 merge 块、循环的块布局）还原，有符号性只在 op 上的值补 `int` / `uint` 转换，显式 location、binding、插值和精度照留。结构体成员、uniform 块要自己起名（`m0`、`S_<哈希>`、`<实例>_Block`），因为方言里没有这些名字。全部测试着色器写出后都能读回（`cargo test --test glsl`）；在 mlirpass 的 `tests/gl.rs` 里，每个读入再写出的着色器在本机 OpenGL 上的渲染都和原始源码逐位一致（Mesa 对源码做了重结合的用例，与 glslang 的 SPIR-V 逐位一致）。

crate 根下只放各格式和使用者共用的组件：`Read` / `Write` 两个 trait、`Pool`、`Binaries`，以及两种树（写出端遍历的只读 `Ir`，读入端在其上构建的可变树）。每种格式在 `src/types/<格式>/` 下各占一个目录。

`Ir` 是各格式共用的只读索引：mlirformat op 树里每个 op、值、块、区域编号，操作数和后继按名字解析好。

## Features

| feature | 缺省 | 是什么 |
|---|---|---|
| `std` | 是 | 标准库（不开时 crate 是 `no_std`，需要 `alloc`），以及 mlirformat 的 `std` |

## 许可

MIT
