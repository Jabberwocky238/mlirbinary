//! AMD RDNA2 (gfx1036) machine code and command streams: each GFX10 instruction op encoded, a
//! kernel's code with what launching it needs, PM4 packets, and an image's IB and code
//! (`Rdna2Image`).

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use mlirformat::isa::{IsaInt, ShaderStage};
use mlirformat::rdna2::{Pm4Nop, Pm4Space, RDNA2, Rdna2Fields as Fields, Rdna2Flag as Flag, Rdna2Format as Format, Rdna2Int as Int, Rdna2Kernel, Rdna2Operand as Operand, Rdna2Reg as Reg};
use mlirformat::{Attribute, Error, NamedAttr, Op, Operation};

use crate::{ImageTarget, Ir, OpId, Pack, Pool, Write};

/// The GFX10 encoder
pub struct Rdna2Isa;

impl Rdna2Isa {
    /// Instruction op `name` with attributes `attrs` and `results` results, as dwords
    pub fn encode(name: &str, attrs: &[NamedAttr<'_>], results: usize) -> Result<Vec<u32>, Error> {
        let i = Fields::of(name, attrs, results)?.ok_or_else(|| Error(format!("{name}: not an RDNA2 instruction")))?;
        Rdna2Words::try_from(&i).map(|w| w.0)
    }
}

/// An instruction's dwords, its literal last
pub struct Rdna2Words(pub Vec<u32>);

impl Rdna2Words {
    fn code(o: Option<Operand>, default: u32) -> u32 {
        o.map_or(default, |o| o.code)
    }
}

impl TryFrom<&Fields> for Rdna2Words {
    type Error = Error;

    fn try_from(i: &Fields) -> Result<Rdna2Words, Error> {
        let code = Rdna2Words::code;
        let (op, c) = (i.op, i.op.code());
        let r = |x: Reg| i.reg(x);
        let need = |x: Reg| r(x).map(|o| o.code).ok_or_else(|| Error(format!("{op:?} needs {x:?}")));
        let n = |x: Int| i.int(x).unwrap_or(0);
        let simm16 = n(Int::Simm16) as u32 & 0xffff;
        let (dw, sources) = match op.format() {
            Format::Sop2 => (vec![(0b10 << 30) | (c << 23) | (code(r(Reg::Dst), 0) << 16) | (code(r(Reg::Src1), 0) << 8) | code(r(Reg::Src0), 0)], [Reg::Src0, Reg::Src1].as_slice()),
            Format::Sopk => (vec![(0b1011 << 28) | (c << 23) | (code(r(Reg::Dst).or(r(Reg::Src0)), 0) << 16) | simm16], [].as_slice()),
            Format::Sop1 => (vec![(0b1_0111_1101 << 23) | (code(r(Reg::Dst), 0) << 16) | (c << 8) | code(r(Reg::Src0), 0)], [Reg::Src0].as_slice()),
            Format::Sopc => (vec![(0b1_0111_1110 << 23) | (c << 16) | (code(r(Reg::Src1), 0) << 8) | code(r(Reg::Src0), 0)], [Reg::Src0, Reg::Src1].as_slice()),
            Format::Sopp => (vec![(0b1_0111_1111 << 23) | (c << 16) | simm16], [].as_slice()),
            Format::Smem => {
                let w0 = (0b11_1101 << 26) | (c << 18) | (u32::from(i.flag(Flag::Glc)) << 16) | (u32::from(i.flag(Flag::Dlc)) << 14) | (code(r(Reg::Dst).or(r(Reg::Data)), 0) << 6) | (need(Reg::Sbase)? >> 1);
                (vec![w0, (code(r(Reg::Soffset), Operand::NULL) << 25) | (n(Int::Offset) as u32 & 0x1f_ffff)], [].as_slice())
            }
            Format::Vop2 if !i.flag(Flag::Vop3) => (vec![(c << 25) | ((need(Reg::Dst)? & 0xff) << 17) | ((need(Reg::Src1)? & 0xff) << 9) | need(Reg::Src0)?], [Reg::Src0, Reg::Src1].as_slice()),
            Format::Vop1 if !i.flag(Flag::Vop3) => (vec![(0b011_1111 << 25) | ((code(r(Reg::Dst), 0) & 0xff) << 17) | (c << 9) | code(r(Reg::Src0), 0)], [Reg::Src0].as_slice()),
            Format::Vopc if !i.flag(Flag::Vop3) => (vec![(0b011_1110 << 25) | (c << 17) | ((need(Reg::Src1)? & 0xff) << 9) | need(Reg::Src0)?], [Reg::Src0, Reg::Src1].as_slice()),
            f @ (Format::Vop1 | Format::Vop2 | Format::Vopc | Format::Vop3) => {
                let c = c + match f {
                    Format::Vop2 => 0x100,
                    Format::Vop1 => 0x180,
                    _ => 0,
                };
                let srcs = [r(Reg::Src0), r(Reg::Src1), r(Reg::Src2)];
                let bits = |f: fn(&Operand) -> bool| srcs.iter().enumerate().map(|(k, o)| u32::from(o.as_ref().is_some_and(f)) << k).sum::<u32>();
                let sdst = r(Reg::Sdst).map_or(0, |o| o.code << 8);
                let w0 = (0b11_0101 << 26) | (c << 16) | (u32::from(i.flag(Flag::Clamp)) << 15) | (bits(|o| o.abs) << 8) | sdst | (code(r(Reg::Dst), 0) & 0xff);
                let w1 = (bits(|o| o.neg) << 29) | ((n(Int::Omod) as u32 & 3) << 27) | (code(srcs[2], 128) << 18) | (code(srcs[1], 128) << 9) | code(srcs[0], 128);
                (vec![w0, w1], [Reg::Src0, Reg::Src1, Reg::Src2].as_slice())
            }
            f @ (Format::Global | Format::Flat | Format::Scratch) => {
                let seg = match f {
                    Format::Scratch => 1,
                    Format::Global => 2,
                    _ => 0,
                };
                let offset = n(Int::Offset);
                if f == Format::Flat && offset != 0 || !(-2048..=2047).contains(&offset) {
                    return Err(Error(format!("{op:?}: offset {offset}")));
                }
                let w0 = (0b11_0111 << 26) | (c << 18) | (u32::from(i.flag(Flag::Slc)) << 17) | (u32::from(i.flag(Flag::Glc)) << 16) | (seg << 14) | (u32::from(i.flag(Flag::Dlc)) << 12) | (offset as u32 & 0xfff);
                let w1 = ((code(r(Reg::Dst), Operand::VGPR) & 0xff) << 24) | (code(r(Reg::Saddr), Operand::NULL) << 16) | ((code(r(Reg::Data), Operand::VGPR) & 0xff) << 8) | (need(Reg::Addr)? & 0xff);
                (vec![w0, w1], [].as_slice())
            }
            Format::Ds => {
                let w0 = (0b11_0110 << 26) | (c << 18) | (u32::from(i.flag(Flag::Gds)) << 17) | ((n(Int::Offset1) as u32 & 0xff) << 8) | (n(Int::Offset0) as u32 & 0xffff);
                let v = |x: Reg| code(r(x), Operand::VGPR) & 0xff;
                (vec![w0, (v(Reg::Dst) << 24) | (v(Reg::Data1) << 16) | (v(Reg::Data0) << 8) | v(Reg::Addr)], [].as_slice())
            }
            f => return Err(Error(format!("{op:?}: {f:?} is not encoded yet"))),
        };
        let mut literal = None;
        for l in sources.iter().filter_map(|s| r(*s)?.literal) {
            match literal {
                Some(m) if m != l => return Err(Error(format!("{op:?}: two different literals"))),
                _ => literal = Some(l),
            }
        }
        let mut dw = dw;
        dw.extend(literal);
        Ok(Rdna2Words(dw))
    }
}


/// PM4 type-3 packets as dwords
pub struct Rdna2Pm4;

impl Rdna2Pm4 {
    /// A one-dword NOP (Mesa's `PKT3_NOP_PAD`): a `nop` packet without a body
    const NOP_PAD: u32 = 0xffff_1000;

    fn header(opcode: u32, count: u32, predicate: bool, compute: bool) -> u32 {
        (3 << 30) | ((count & 0x3fff) << 16) | ((opcode & 0xff) << 8) | (u32::from(compute) << 1) | u32::from(predicate)
    }

    fn addr(ir: &Ir<'_, '_>, op: OpId) -> Result<u32, Error> {
        RDNA2.get(ir[op].name).map(|i| i.num).ok_or_else(|| Error(format!("{}: no such register", ir[op].name)))
    }

    /// Packet op `op`: a `SET_*_REG` packet with its registers, or a packet with its body
    pub fn pack(ir: &Ir<'_, '_>, op: OpId) -> Result<Vec<u32>, Error> {
        let name = ir[op].name;
        if let Some(space) = Pm4Space::named(name) {
            let regs = ir.block_ops(ir.top_block(op));
            let first = Rdna2Pm4::addr(ir, *regs.first().ok_or_else(|| Error(format!("{name} without registers")))?)?;
            let mut dw = vec![Rdna2Pm4::header(Pm4Space::opcode(name)?, regs.len() as u32, false, false), (first - space.start) >> 2];
            for r in regs {
                dw.extend(Pack(RDNA2).pack(ir[r].name, &ir[r].all_attrs())?);
            }
            return Ok(dw);
        }
        if name.starts_with(Pm4Space::REG) {
            return Err(Error(format!("{name} outside a SET_*_REG packet: run rdna2-pm4")));
        }
        let attrs = ir[op].all_attrs();
        let flag = |n: &str| attrs.iter().any(|x| x.name == n && x.value.as_ref().is_none_or(|v| matches!(v, Attribute::Bool(true) | Attribute::Unit)));
        let body = match attrs.iter().find(|a| a.name == "body").and_then(|a| a.value.as_ref()) {
            None => &[][..],
            Some(Attribute::Array(b)) => &b[..],
            Some(a) => return Err(Error(format!("{name}: body = {a}"))),
        };
        let mut words = Vec::new();
        for e in body {
            match e {
                Attribute::Array(q) if q.len() == 1 => {
                    let v = IsaInt::try_from(&q[0])?.0 as u64;
                    words.extend([v as u32, (v >> 32) as u32]);
                }
                _ => words.push(u32::try_from(IsaInt::try_from(e)?.0).map_err(|_| Error(format!("{name}: {e} is not a dword (a qword is [v])")))?),
            }
        }
        let head = match words.len() {
            0 if name == Pm4Nop::NAME => return Ok(vec![Rdna2Pm4::NOP_PAD]),
            0 => return Err(Error(format!("{name}: a packet has a body"))),
            n => Rdna2Pm4::header(Pm4Space::opcode(name)?, n as u32 - 1, flag("predicate"), flag("compute")),
        };
        Ok([vec![head], words].concat())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rdna2Code {
    pub name: String,
    pub stage: Option<ShaderStage>,
    pub params: u32,
    pub sgprs: u32,
    pub vgprs: u32,
    pub pad: u64,
    pub align: u64,
    pub bytes: Vec<u8>,
}

impl Rdna2Code {
    /// An `rdna2.kernel`'s code
    pub fn of(kernel: &Operation<'_>) -> Result<Rdna2Code, Error> {
        let head = Rdna2Kernel::try_from(kernel)?;
        let ir = Ir::new(kernel)?;
        let mut bytes = Vec::new();
        for op in ir.block_ops(ir.top_block(ir.root)) {
            let dw = Rdna2Isa::encode(ir[op].name, &ir[op].all_attrs(), ir[op].op.ty.results.len())?;
            bytes.extend(dw.iter().flat_map(|d| d.to_le_bytes()));
        }
        let name = match &head.sym_name {
            Attribute::SymbolRef(p) => p[0].trim_start_matches('@').to_owned(),
            a @ Attribute::Str { .. } => a.text()?,
            other => return Err(Error(format!("{other} is not a symbol"))),
        };
        Ok(Rdna2Code {
            name,
            stage: head.stage,
            params: head.params.unwrap_or(0),
            sgprs: head.sgprs.unwrap_or(0),
            vgprs: head.vgprs.unwrap_or(0),
            pad: head.pad.unwrap_or(0),
            align: head.align.unwrap_or(1),
            bytes,
        })
    }
}

/// The `rdna2` machine code of a kernel (`rdna2.kernel`), or of every kernel in an op, one after
/// the other; `Rdna2Code::of` gives one kernel's code with its launch parameters
#[derive(Debug, Clone, Copy, Default)]
pub struct Rdna2Writer;

impl Rdna2Writer {
    pub const NAME: &'static str = "rdna2";

    /// Every `rdna2.kernel` in `op` (`op` itself if it is one)
    pub fn kernels(op: &Operation<'_>) -> Result<Vec<Rdna2Code>, Error> {
        let mut out = Vec::new();
        Self::collect(op, &mut out)?;
        Ok(out)
    }

    fn collect(op: &Operation<'_>, out: &mut Vec<Rdna2Code>) -> Result<(), Error> {
        if op.name == Rdna2Kernel::NAME {
            out.push(Rdna2Code::of(op)?);
            return Ok(());
        }
        for o in op.regions.iter().flat_map(|r| &r.blocks).flat_map(|b| &b.ops) {
            Self::collect(o, out)?;
        }
        Ok(())
    }
}

impl Write for Rdna2Writer {
    fn name(&self) -> &'static str {
        Rdna2Writer::NAME
    }

    fn write(&self, op: &Operation<'_>, _pool: &dyn Pool) -> Result<Vec<u8>, Error> {
        Ok(Rdna2Writer::kernels(op)?.into_iter().flat_map(|k| k.bytes).collect())
    }
}

/// An RDNA2 image: memories from 16 MiB up in 64 KiB pages, an IB of PM4 packets, kernels padded
#[derive(Debug, Clone, Copy, Default)]
pub struct Rdna2Image;

impl ImageTarget for Rdna2Image {
    const BASE: u64 = 0x100_0000;
    const PAGE: u64 = 0x1_0000;

    fn stream(&self, ir: &Ir<'_, '_>, stream: OpId) -> Result<Vec<u8>, Error> {
        let mut dw = Vec::new();
        for op in ir.block_ops(ir.top_block(stream)) {
            dw.extend(Rdna2Pm4::pack(ir, op)?);
        }
        Ok(dw.iter().flat_map(|d| d.to_le_bytes()).collect())
    }

    fn item(&self, ir: &Ir<'_, '_>, op: OpId) -> Result<Vec<u8>, Error> {
        if ir[op].name != Rdna2Kernel::NAME {
            return Err(Error(format!("{} cannot be placed in memory", ir[op].name)));
        }
        let code = Rdna2Code::of(ir[op].op)?;
        let mut bytes = code.bytes;
        bytes.resize(bytes.len() + code.pad as usize, 0);
        Ok(bytes)
    }
}
