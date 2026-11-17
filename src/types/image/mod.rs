//! A GPU image's bytes: the records of a GPU dialect packed into dwords by its layout tables
//! (`Pack`), and an image laid out by mlirpass written as its memories' contents (`ImageWriter`,
//! each GPU giving its own command streams and records through `ImageTarget`).

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use mlirformat::image::{Data, Field, Item, Layouts, MemType, Memory, Placed, Stream, Ty, Words};
use mlirformat::{Attribute, Const, Error, NamedAttr, Op, Operation};

use crate::{Ir, OpId};

/// What a GPU gives an image: where its memories start and how they are paged, and the bytes of
/// its command streams and of its records
pub trait ImageTarget {
    const BASE: u64;
    const PAGE: u64;

    fn stream(&self, ir: &Ir<'_, '_>, stream: OpId) -> Result<Vec<u8>, Error>;
    fn item(&self, ir: &Ir<'_, '_>, op: OpId) -> Result<Vec<u8>, Error>;

    /// An item's bytes: data and words as they are, the rest the GPU's
    fn contents(&self, ir: &Ir<'_, '_>, op: OpId) -> Result<Vec<u8>, Error> {
        let name = ir[op].name;
        if name == Stream::NAME {
            self.stream(ir, op)
        } else if name == Data::NAME {
            let d = Data::try_from(ir[op].op)?;
            let mut bytes = d.bytes.as_ref().map(Image::dense).transpose()?.unwrap_or_default();
            bytes.resize(bytes.len().max(d.size.unwrap_or(0) as usize), 0);
            Ok(bytes)
        } else if name == Words::NAME {
            let w = Words::try_from(ir[op].op)?;
            w.values.iter().map(|v| v.int().map(|v| (v as u32).to_le_bytes())).collect::<Result<Vec<_>, _>>().map(|w| w.concat())
        } else {
            self.item(ir, op)
        }
    }
}

/// A memory of an image: its address, kind and bytes
#[derive(Debug, Clone)]
pub struct Segment {
    pub name: String,
    pub addr: u64,
    pub mem: MemType,
    pub bytes: Vec<u8>,
}

/// An image: its memories, and where its first command stream starts and ends
#[derive(Debug, Clone)]
pub struct Image {
    pub segments: Vec<Segment>,
    pub stream: u64,
    pub stream_end: u64,
}

impl Image {
    pub fn segment(&self, name: &str) -> Option<&Segment> {
        self.segments.iter().find(|s| s.name == name)
    }

    /// A dense constant's elements, little-endian
    pub fn dense(a: &Attribute<'_>) -> Result<Vec<u8>, Error> {
        let c = Const::try_from(a).map_err(|()| Error("bytes: not a dense constant".into()))?;
        let n = c.ty.element_count().unwrap_or(1);
        let width = c.ty.element_type().bit_width().ok_or_else(|| Error("bytes: element without a width".into()))? as usize / 8;
        let mut out = Vec::with_capacity(n * width);
        for i in 0..n {
            let bits = match c.at(i) {
                mlirformat::Scalar::Float(f) => u128::from(f.bits),
                mlirformat::Scalar::Int(v) => v.bits(),
                mlirformat::Scalar::Complex(..) | mlirformat::Scalar::ComplexFloat(..) | mlirformat::Scalar::Wide(_) => {
                    return Err(Error("bytes: complex and wide float elements have no image layout".into()));
                }
            };
            out.extend_from_slice(&bits.to_le_bytes()[..width]);
        }
        Ok(out)
    }

    fn symbol(a: &Attribute<'_>) -> Result<String, Error> {
        match a {
            Attribute::SymbolRef(p) => Ok(p[0].trim_start_matches('@').trim_matches('"').to_owned()),
            Attribute::Str { .. } => a.text(),
            other => Err(Error(format!("{other} is not a symbol"))),
        }
    }
}

/// Writes an image laid out by mlirpass (its memories at their addresses, its items at their offsets)
pub struct ImageWriter<'t, T: ImageTarget>(pub &'t T);

impl<T: ImageTarget> ImageWriter<'_, T> {
    /// The image whose memories `op` (the container of `image.memory` ops) holds
    pub fn image(&self, op: &Operation<'_>) -> Result<Image, Error> {
        let ir = Ir::new(op)?;
        let top = ir.top_block(ir.root);
        let mut segments = Vec::new();
        let mut stream: Option<(u64, u64)> = None;
        for m in ir.block_ops(top).into_iter().filter(|m| ir[*m].name == Memory::NAME) {
            let head = Memory::try_from(ir[m].op)?;
            let name = Image::symbol(&head.sym_name)?;
            let addr = head.addr.ok_or_else(|| Error(format!("memory @{name} without an address: run the layout")))?;
            let size = head.size.unwrap_or(0);
            let mem = head.mem.unwrap_or(MemType::Streaming);
            let mut bytes = vec![0u8; size as usize];
            for item in ir.block_ops(ir.top_block(m)) {
                let offset = ir[item].get(Placed::OFFSET).map(Attribute::int).transpose()?.unwrap_or(0) as usize;
                let b = self.0.contents(&ir, item)?;
                bytes[offset..offset + b.len()].copy_from_slice(&b);
                if ir[item].name == Stream::NAME && stream.is_none() {
                    let at = addr + offset as u64;
                    stream = Some((at, at + b.len() as u64));
                }
            }
            segments.push(Segment { name, addr, mem, bytes });
        }
        let (stream, stream_end) = stream.ok_or_else(|| Error("no image.stream".into()))?;
        Ok(Image { segments, stream, stream_end })
    }
}

/// Packs a record of a GPU dialect's layout tables into dwords
pub struct Pack<'t>(pub Layouts<'t>);

impl<'t> Pack<'t> {
    pub fn pack(&self, op: &str, attrs: &[NamedAttr<'_>]) -> Result<Vec<u32>, Error> {
        let t = self.template(op, attrs)?;
        match t.holes.first() {
            Some(h) => Err(Error(format!("{op}: @{} is not a number yet", h.name))),
            None => Ok(t.dw),
        }
    }

    pub fn template(&self, op: &str, attrs: &[NamedAttr<'_>]) -> Result<Template, Error> {
        let item = self.0.get(op).ok_or_else(|| Error(format!("{op}: not an item of the table")))?;
        let mut p = Packer::new(self.0, item);
        p.pack(attrs)?;
        Ok(Template { dw: p.dw, holes: p.holes })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hole {
    pub name: String,
    bit: u32,
    width: u32,
    shift: u32,
    add: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Template {
    pub dw: Vec<u32>,
    pub holes: Vec<Hole>,
}

impl Template {
    pub fn emit(&self, out: &mut Vec<u32>, value: impl Fn(&str) -> u64) {
        self.emit_indexed(out, |i| value(&self.holes[i].name));
    }

    pub fn emit_indexed(&self, out: &mut Vec<u32>, value: impl Fn(usize) -> u64) {
        let at = out.len();
        out.extend_from_slice(&self.dw);
        for (i, h) in self.holes.iter().enumerate() {
            let v = (value(i) + h.add) >> h.shift;
            let v = if h.width >= 64 { v } else { v & ((1 << h.width) - 1) };
            let mut rest = u128::from(v) << (h.bit % 32);
            for d in &mut out[at + (h.bit / 32) as usize..at + (h.bit + h.width).div_ceil(32) as usize] {
                *d |= rest as u32;
                rest >>= 32;
            }
        }
    }

    pub fn extend(&mut self, other: &Template) {
        let base = self.dw.len() as u32 * 32;
        self.dw.extend_from_slice(&other.dw);
        self.holes.extend(other.holes.iter().map(|h| Hole { bit: h.bit + base, ..h.clone() }));
    }
}

struct Packer<'l, 't> {
    layouts: Layouts<'t>,
    item: &'l Item<'t>,
    dw: Vec<u32>,
    holes: Vec<Hole>,
}

const META: &[&str] = &["elements", "sym_name", "align", "offset"];

impl<'l, 't> Packer<'l, 't> {
    fn new(layouts: Layouts<'t>, item: &'l Item<'t>) -> Self {
        Packer { layouts, item, dw: vec![0; item.length as usize], holes: Vec::new() }
    }

    fn err<T>(&self, msg: impl core::fmt::Display) -> Result<T, Error> {
        Err(Error(format!("{}: {msg}", self.item.op)))
    }

    fn pack(&mut self, attrs: &[NamedAttr<'_>]) -> Result<(), Error> {
        let get = |name: &str| attrs.iter().find(|a| a.name == name).map(|a| a.value.as_ref().unwrap_or(&Attribute::Unit));
        for a in attrs {
            if !META.contains(&&*a.name) && !self.item.fields.iter().any(|f| f.name == a.name) {
                return self.err(format_args!("no field {}", a.name));
            }
        }
        let auto_length = self.item.bias > 0 && get("dword_length").is_none() && self.item.fields.iter().any(|f| f.name == "dword_length");
        for f in self.item.fields {
            if !(auto_length && f.name == "dword_length") {
                self.field(f, 0, get(f.name))?;
            }
        }
        match (self.item.group, get("elements")) {
            (Some(g), Some(Attribute::Array(es))) => {
                self.dw.resize((g.at as usize + es.len() * g.size as usize).div_ceil(32).max(self.dw.len()), 0);
                for (i, e) in es.iter().enumerate() {
                    let Attribute::Dict(es) = e else { return self.err("an element is not a dictionary") };
                    for f in g.fields {
                        let v = es.iter().find(|a| a.name == f.name).map(|a| a.value.as_ref().unwrap_or(&Attribute::Unit));
                        self.field(f, g.at + i as u32 * g.size, v)?;
                    }
                }
            }
            (_, None) => {}
            _ => return self.err("elements: not a variable-length item, or not an array"),
        }
        if auto_length {
            let f = self.item.fields.iter().find(|f| f.name == "dword_length").unwrap();
            let n = self.dw.len() as u64 - u64::from(self.item.bias);
            self.put(f.bit, f.width, n, true);
        }
        Ok(())
    }

    fn field(&mut self, f: &Field<'_>, base: u32, value: Option<&Attribute<'_>>) -> Result<(), Error> {
        self.elements(f, base + f.bit, f.dims, value, value.is_some())
    }

    fn elements(&mut self, f: &Field<'_>, bit: u32, dims: &[(u32, u32)], value: Option<&Attribute<'_>>, set: bool) -> Result<(), Error> {
        let Some(((count, stride), inner)) = dims.split_first() else { return self.scalar(f, bit, value, set) };
        let items = match value {
            Some(Attribute::Array(v)) if v.len() == *count as usize => Some(v),
            None => None,
            _ => return self.err(format_args!("{}: not an array of {count}", f.name)),
        };
        for i in 0..*count {
            self.elements(f, bit + i * stride, inner, items.map(|v| &v[i as usize]), set)?;
        }
        Ok(())
    }

    fn scalar(&mut self, f: &Field<'_>, bit: u32, value: Option<&Attribute<'_>>, set: bool) -> Result<(), Error> {
        let v = match (f.ty, value) {
            (Ty::Struct(op), v) => {
                let Some(item) = self.layouts.get(op) else { return self.err(format_args!("{}: no struct {op}", f.name)) };
                let attrs = match v {
                    Some(Attribute::Dict(a)) => a.as_slice(),
                    None => &[],
                    _ => return self.err(format_args!("{}: not a dictionary", f.name)),
                };
                let mut inner = Packer::new(self.layouts, item);
                inner.pack(attrs)?;
                for (k, w) in inner.dw.iter().enumerate().take(f.width.div_ceil(32) as usize) {
                    let k = k as u32;
                    self.put(bit + 32 * k, (f.width - 32 * k).min(32), u64::from(*w), set);
                }
                self.holes.extend(inner.holes.into_iter().map(|h| Hole { bit: h.bit + bit, ..h }));
                return Ok(());
            }
            (Ty::Uint, Some(Attribute::Array(words))) if f.width > 64 => {
                for (k, w) in words.iter().enumerate() {
                    let (k, w) = (k as u32, Scalar::try_from(w)?.0 as u64);
                    self.put(bit + 32 * k, (f.width - 32 * k).min(32), w, set);
                }
                return Ok(());
            }
            (Ty::Mbo, Some(_)) => return self.err(format_args!("{}: must be one, not settable", f.name)),
            (Ty::Exact, Some(_)) => return self.err(format_args!("{}: fixed to {:#x}, not settable", f.name, f.default)),
            (ty, Some(Attribute::SymbolRef(p))) => return self.hole(f, ty, bit, p, 0),
            (ty, Some(Attribute::Array(v))) if matches!(&v[..], [Attribute::SymbolRef(_), Attribute::Int { .. }]) => {
                let (Attribute::SymbolRef(p), add) = (&v[0], Scalar::try_from(&v[1])?.0 as u64) else { unreachable!() };
                return self.hole(f, ty, bit, p, add);
            }
            (_, None) => f.default,
            (ty, Some(a)) => {
                let s = Scalar::try_from(a)?;
                match Self::encode(ty, s, f.width, bit % 32) {
                    Some(v) => v,
                    None => return self.err(format_args!("{}: {a} does not fit {ty:?} of {} bits at bit {}", f.name, f.width, bit % 32)),
                }
            }
        };
        self.put(bit, f.width, v, set);
        Ok(())
    }

    fn hole(&mut self, f: &Field<'_>, ty: Ty<'_>, bit: u32, path: &[mlirformat::Str<'_>], add: u64) -> Result<(), Error> {
        let shift = match ty {
            Ty::Uint | Ty::Int | Ty::Bool => 0,
            Ty::Address => bit % 32,
            Ty::Shr(n) => n.into(),
            _ => return self.err(format_args!("{}: a symbol in a {ty:?} field", f.name)),
        };
        let name = path.iter().map(|s| s.trim_matches('"')).collect::<Vec<_>>().join("::");
        self.holes.push(Hole { name, bit, width: f.width, shift, add });
        Ok(())
    }

    fn put(&mut self, bit: u32, width: u32, v: u64, set: bool) {
        let v = if width >= 64 { v } else { v & ((1 << width) - 1) };
        let mut rest = u128::from(v) << (bit % 32);
        for k in (bit / 32) as usize..(bit + width).div_ceil(32) as usize {
            let d = rest as u32;
            rest >>= 32;
            if k >= self.dw.len() {
                if !set || d == 0 {
                    continue;
                }
                self.dw.resize(k + 1, 0);
            }
            self.dw[k] |= d;
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Scalar(i128, Option<f64>);

impl TryFrom<&Attribute<'_>> for Scalar {
    type Error = Error;

    fn try_from(a: &Attribute<'_>) -> Result<Scalar, Error> {
        let int = |lit: &str| {
            let (neg, l) = lit.strip_prefix('-').map_or((false, lit), |l| (true, l));
            let v = match l.strip_prefix("0x") {
                Some(h) => i128::from_str_radix(h, 16),
                None => l.parse(),
            };
            v.ok().map(|v| if neg { -v } else { v })
        };
        match a {
            Attribute::Unit => Ok(Scalar(1, None)),
            Attribute::Bool(b) => Ok(Scalar((*b).into(), None)),
            Attribute::Int { lit, .. } => int(lit).map(|v| Scalar(v, Some(v as f64))).ok_or_else(|| Error(format!("bad integer {lit}"))),
            Attribute::Float { lit, .. } => lit.parse::<f64>().map(|v| Scalar(v as i128, Some(v))).map_err(|_| Error(format!("bad float {lit}"))),
            _ => Err(Error(format!("{a} is not a number"))),
        }
    }
}

impl Packer<'_, '_> {
    /// A value as a field of type `ty` encodes it
    fn encode(ty: Ty<'_>, s: Scalar, width: u32, lo: u32) -> Option<u64> {
        let fits = |v: i128| v >= 0 && (width >= 64 || v >> width == 0);
        let frac = |v: f64, f: u8| libm::round(v * (1u64 << f) as f64) as i128;
        match ty {
            Ty::Bool => (s.0 == 0 || s.0 == 1).then_some(s.0 as u64),
            Ty::Uint => fits(s.0).then_some(s.0 as u64),
            Ty::Int => (s.0 >= -(1i128 << (width - 1)) && s.0 < 1i128 << width).then_some(s.0 as u64),
            Ty::Float => Some(u64::from((s.1? as f32).to_bits())),
            Ty::Ufixed(f) => Some(frac(s.1?, f)).filter(|v| fits(*v)).map(|v| v as u64),
            Ty::Sfixed(f) => Some(frac(s.1?, f) as u64),
            Ty::Address => (s.0 >= 0 && s.0 & ((1 << lo) - 1) == 0 && (width + lo >= 64 || s.0 >> (width + lo) == 0)).then_some((s.0 >> lo) as u64),
            Ty::Shr(n) => (s.0 >= 0 && s.0 & ((1 << n) - 1) == 0).then_some(s.0 >> n).filter(|v| fits(*v)).map(|v| v as u64),
            Ty::Minus(n) => Some(s.0 - i128::from(n)).filter(|v| fits(*v)).map(|v| v as u64),
            Ty::Log2 => (s.0 > 0 && s.0 & (s.0 - 1) == 0).then(|| i128::from(s.0.trailing_zeros())).filter(|v| fits(*v)).map(|v| v as u64),
            Ty::Groups(n) => {
                let groups = if s.0 == 0 { 1 } else { s.0.checked_add(i128::from(n) - 1)? / i128::from(n) };
                match groups {
                    g if g == 1 << width => Some(0),
                    g => fits(g).then_some(g as u64),
                }
            }
            Ty::Lod => Some(u64::from((s.1? as f32 * 64.0).clamp(0.0, 896.0) as u32)),
            Ty::Half => Some(u64::from(Half::from(s.1? as f32).0)),
            Ty::Struct(_) | Ty::Mbo | Ty::Exact => None,
        }
    }
}

struct Half(u16);

impl From<f32> for Half {
    fn from(f: f32) -> Half {
        let bits = f.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let (exp, man) = ((bits >> 23) & 0xff, bits & 0x7f_ffff);
        if exp == 0xff {
            return Half(sign | 0x7c00 | if man != 0 { 0x200 | (man >> 13) as u16 } else { 0 });
        }
        let x = f64::from(f.abs());
        if x < libm::ldexp(1.0, -14) {
            return Half(sign | libm::rint(x * libm::ldexp(1.0, 24)) as u16);
        }
        let e = exp as i32 - 127 + 15;
        if e >= 31 {
            return Half(sign | 0x7c00);
        }
        let full = ((e as u32) << 23) | man;
        let (mut h, rest) = (full >> 13, full & 0x1fff);
        if rest > 0x1000 || (rest == 0x1000 && h & 1 == 1) {
            h += 1;
        }
        Half(sign | h.min(0x7c00) as u16)
    }
}

