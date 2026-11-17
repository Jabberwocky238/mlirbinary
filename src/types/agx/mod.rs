//! Apple AGX (G13) machine code and command buffers: each USC instruction of the `agx` dialect
//! encoded by its table, a kernel's code with what launching it needs, the command-buffer structs
//! packed by their layouts, and an image's command streams and records (`AgxImage`).

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use mlirformat::agx::{AGX, AgxKernel, Usc, UscKind};
use mlirformat::isa::ShaderStage;
use mlirformat::{Attribute, Error, NamedAttr, Op, Operation};

use crate::{ImageTarget, Ir, OpId, Pack, Pool, Write};

/// The USC encoder: an instruction's fields and operands placed by its table
pub struct AgxIsa;

impl AgxIsa {
    /// Bytes an encoding is at most (`u128`)
    const WIDEST: u32 = 16;

    fn operand(enc: mlirformat::agx::Enc, a: &Attribute<'_>) -> Result<u128, Error> {
        let kind = enc.name();
        let Attribute::Dict(d) = a else { return Err(Error(format!("{kind}: {a} is not an operand dictionary"))) };
        let k = d.iter().find(|n| n.name == "case").and_then(|n| n.value.as_ref()).ok_or_else(|| Error(format!("{kind}: no case")))?;
        let k = mlirformat::agx::UscField::int(k)? as usize;
        let case = enc.cases().get(k).ok_or_else(|| Error(format!("{kind} has no case {k}")))?;
        let mut v = case.value;
        for n in d {
            let f = match &*n.name {
                "case" => continue,
                name => case.field.iter().chain(case.modifiers).find(|f| f.name == name),
            };
            let f = f.ok_or_else(|| Error(format!("{kind} case {k} has no {}", n.name)))?;
            let x = n.value.as_ref().map_or(Ok(1), |x| f.raw(x))?;
            v |= f.deposit(x)?;
        }
        if v & case.mask != case.value {
            return Err(Error(format!("{kind} case {k}: a field sets its exact bits")));
        }
        Ok(v)
    }

    /// Instruction `op` with fields `attrs` encoded: its short form unless it says `long` or does not fit
    pub fn encode(op: &str, attrs: &[NamedAttr<'_>]) -> Result<Vec<u8>, Error> {
        let ins = Usc::get(op).ok_or_else(|| Error(format!("{op}: not a USC instruction")))?;
        let mut v = ins.value;
        let mut long = false;
        for n in attrs {
            let name = &*n.name;
            if name == "long" {
                long = true;
                continue;
            }
            let index = |prefix: &str| name.strip_prefix(prefix).and_then(|k| k.parse::<usize>().ok());
            let (field, operand) = match (index("d"), index("s")) {
                (Some(k), _) => (ins.dests.get(k), true),
                (_, Some(k)) => (ins.sources.get(k), true),
                _ => (ins.modifiers.iter().chain(ins.immediates).find(|f| f.name == name), false),
            };
            let f = field.ok_or_else(|| Error(format!("{op} has no {name}")))?;
            let x = match (&n.value, operand) {
                (Some(a), true) => match f.kind {
                    UscKind::Operand(e) => AgxIsa::operand(e, a)?,
                    k => return Err(Error(format!("{op}: {name} is a {k:?}, not an operand"))),
                },
                (Some(a), false) => f.raw(a)?,
                (None, false) => 1,
                (None, true) => return Err(Error(format!("{op}: {name} without a value"))),
            };
            v |= f.deposit(x)?;
        }
        if v & ins.mask != ins.value {
            return Err(Error(format!("{op}: a field sets its exact bits")));
        }
        let fits = |v: u128, n: u32| n >= AgxIsa::WIDEST || v >> (n * 8) == 0;
        let (n, v) = match (ins.lengths, ins.length_bit) {
            ([short, l], Some(bit)) if long || !fits(v, *short) => (*l, v | 1 << bit),
            _ if long => return Err(Error(format!("{op} has no long form"))),
            ([short, ..], _) => (*short, v),
            ([], _) => return Err(Error(format!("{op}: no length"))),
        };
        if !fits(v, n) {
            return Err(Error(format!("{op}: bits past its {n} bytes")));
        }
        Ok(v.to_le_bytes()[..n as usize].to_vec())
    }
}

/// A command-buffer struct (`agx.cdm_launch_word_0`, …) packed by its layout
pub struct AgxRecord;

impl AgxRecord {
    pub fn bytes(name: &str, attrs: &[NamedAttr<'_>]) -> Result<Vec<u8>, Error> {
        let item = AGX.get(name).ok_or_else(|| Error(format!("{name} is not an agx struct")))?;
        let dw = Pack(AGX).pack(name, attrs)?;
        Ok(dw.iter().flat_map(|d| d.to_le_bytes()).take(item.num as usize).collect())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgxCode {
    pub name: String,
    pub stage: Option<ShaderStage>,
    pub params: u32,
    pub gprs: u32,
    pub align: u64,
    pub bytes: Vec<u8>,
}

impl AgxCode {
    /// An `agx.kernel`'s code
    pub fn of(kernel: &Operation<'_>) -> Result<AgxCode, Error> {
        let head = AgxKernel::try_from(kernel)?;
        let ir = Ir::new(kernel)?;
        let mut bytes = Vec::new();
        for op in ir.block_ops(ir.top_block(ir.root)) {
            bytes.extend(AgxIsa::encode(ir[op].name, &ir[op].all_attrs())?);
        }
        let name = match &head.sym_name {
            Attribute::SymbolRef(p) => p[0].trim_start_matches('@').to_owned(),
            a @ Attribute::Str { .. } => a.text()?,
            other => return Err(Error(format!("{other} is not a symbol"))),
        };
        Ok(AgxCode { name, stage: head.stage, params: head.params.unwrap_or(0), gprs: head.gprs.unwrap_or(0), align: head.align.unwrap_or(1), bytes })
    }
}

/// The `agx` machine code of a kernel (`agx.kernel`), or of every kernel in an op, one after the
/// other; `AgxCode::of` gives one kernel's code with its launch parameters
#[derive(Debug, Clone, Copy, Default)]
pub struct AgxWriter;

impl AgxWriter {
    pub const NAME: &'static str = "agx";

    /// Every `agx.kernel` in `op` (`op` itself if it is one)
    pub fn kernels(op: &Operation<'_>) -> Result<Vec<AgxCode>, Error> {
        let mut out = Vec::new();
        Self::collect(op, &mut out)?;
        Ok(out)
    }

    fn collect(op: &Operation<'_>, out: &mut Vec<AgxCode>) -> Result<(), Error> {
        if op.name == AgxKernel::NAME {
            out.push(AgxCode::of(op)?);
            return Ok(());
        }
        for o in op.regions.iter().flat_map(|r| &r.blocks).flat_map(|b| &b.ops) {
            Self::collect(o, out)?;
        }
        Ok(())
    }
}

impl Write for AgxWriter {
    fn name(&self) -> &'static str {
        AgxWriter::NAME
    }

    fn write(&self, op: &Operation<'_>, _pool: &dyn Pool) -> Result<Vec<u8>, Error> {
        Ok(AgxWriter::kernels(op)?.into_iter().flat_map(|k| k.bytes).collect())
    }
}

/// An AGX image: memories from 4 GiB up in 16 KiB pages, command streams of packed structs
#[derive(Debug, Clone, Copy, Default)]
pub struct AgxImage;

impl ImageTarget for AgxImage {
    const BASE: u64 = 1 << 32;
    const PAGE: u64 = 0x4000;

    fn stream(&self, ir: &Ir<'_, '_>, stream: OpId) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        for op in ir.block_ops(ir.top_block(stream)) {
            bytes.extend(AgxRecord::bytes(ir[op].name, &ir[op].all_attrs())?);
        }
        Ok(bytes)
    }

    fn item(&self, ir: &Ir<'_, '_>, op: OpId) -> Result<Vec<u8>, Error> {
        match ir[op].name {
            n if n == AgxKernel::NAME => Ok(AgxCode::of(ir[op].op)?.bytes),
            n => AgxRecord::bytes(n, &ir[op].all_attrs()),
        }
    }
}
