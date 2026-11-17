//! AMD RDNA2 machine code and registers: instructions encoded as llvm-mc assembles them, registers
//! packed as Mesa's `S_*` macros pack them

use mlirbinary::{Pack, Rdna2Isa};
use mlirformat::rdna2::{RDNA2, Rdna2Dialect, Rdna2Format as Format, Rdna2Regs, Rdna2Valu as Valu};
use mlirformat::{Attribute, Item, MlirOpt, Module, NamedAttr, Operation};

/// An op's attributes, inherent and not
fn attrs<'a>(o: &Operation<'a>) -> Vec<NamedAttr<'a>> {
    let mut out = match &o.properties {
        Some(Attribute::Dict(p)) => p.clone(),
        _ => Vec::new(),
    };
    out.extend(o.attributes.iter().cloned());
    out
}

fn ops<'a>(m: Module<'a>) -> Vec<Operation<'a>> {
    m.items.into_iter().filter_map(|i| if let Item::Op(o) = i { Some(o) } else { None }).collect()
}

/// `src`'s ops, and its ops parsed again from how it prints, to `f`
fn twice(src: &str, f: impl FnOnce(&[Operation<'_>], &[Operation<'_>])) {
    let mut opt = MlirOpt::upstream();
    opt.load(Rdna2Dialect);
    let module = opt.parse(src).unwrap();
    let printed = opt.show(&module).to_string();
    f(&ops(module), &ops(opt.parse(&printed).unwrap()));
}

/// `fixtures/isa.mlir` encodes as llvm-mc assembles the comment above each op
/// (`fixtures/isa.expected`), but for the fields of VOP3 sources an op leaves out
#[test]
fn encodes_as_llvm_mc() {
    let src = include_str!("fixtures/isa.mlir");
    twice(src, |ops, reread| {
        let asm: Vec<&str> = src.lines().filter_map(|l| l.strip_prefix("// ")).filter(|l| l.starts_with(|c: char| c.is_ascii_lowercase())).collect();
        let expected: Vec<&str> = include_str!("fixtures/isa.expected").lines().collect();
        assert_eq!(ops.len(), expected.len());
        let mut bad = Vec::new();
        for (((op, re), want), asm) in ops.iter().zip(reread).zip(&expected).zip(&asm) {
            let unused: u32 = match Valu::try_from(op) {
                Ok(v) if v.vop3 || v.op.format() == Format::Vop3 => [(v.src1, 0x1ff << 9), (v.src2, 0x1ff << 18)].iter().filter(|(s, _)| s.is_none()).map(|(_, m)| m).sum(),
                _ => 0,
            };
            let clear = |mut dw: Vec<u32>| {
                if dw.len() > 1 {
                    dw[1] &= !unused;
                }
                dw.iter().flat_map(|d| d.to_le_bytes()).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
            };
            let want = &clear(want.split(' ').collect::<Vec<_>>().chunks(4).map(|b| u32::from_le_bytes(std::array::from_fn(|i| u8::from_str_radix(b[i], 16).unwrap()))).collect());
            let encode = |o: &Operation<'_>| Rdna2Isa::encode(&o.name, &attrs(o), o.ty.results.len()).map_or_else(|e| e.to_string(), clear);
            let (got, got_again) = (encode(op), encode(re));
            if got != *want || got_again != got {
                bad.push(format!("{asm}\n want {want}\n  got {got}\n printed and read again {got_again}"));
            }
        }
        assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), expected.len(), bad.join("\n"));
    });
}

/// `fixtures/oracle.mlir` (every register with fields, random values) packs as Mesa's `S_*` macros
/// pack the same values (`fixtures/oracle.expected`)
#[test]
fn registers_pack_as_mesa() {
    twice(include_str!("fixtures/oracle.mlir"), |ops, reread| {
        let expected: Vec<&str> = include_str!("fixtures/oracle.expected").lines().collect();
        assert_eq!(ops.len(), expected.len());
        let pack = |o: &Operation<'_>| {
            let item = RDNA2.get(&o.name).unwrap();
            let dw = Pack(RDNA2).pack(&o.name, &attrs(o)).unwrap_or_else(|e| panic!("{e}"));
            format!("{} {:08x}", item.name, dw[0])
        };
        let mut bad = Vec::new();
        for ((op, re), want) in ops.iter().zip(reread).zip(&expected) {
            let (got, got_again) = (pack(op), pack(re));
            let typed = Rdna2Regs::verify(op).map(|r| r.map_err(|e| e.to_string()));
            if got != *want || got_again != got || !matches!(typed, Some(Ok(()))) {
                bad.push(format!("want {want}\n got {got}\n printed and read again {got_again}\n typed {typed:?}"));
            }
        }
        assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), expected.len(), bad.join("\n"));
    });
}
