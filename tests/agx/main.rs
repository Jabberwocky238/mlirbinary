//! Apple AGX machine code and command buffers: USC instructions encoded as Mesa's disassembler
//! cases are, structs packed as Mesa's `agx_pack` packs them

use mlirbinary::{AgxIsa, Pack};
use mlirformat::agx::{AGX, AgxDialect, AgxStructs, Usc};
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
    m.items
        .into_iter()
        .filter_map(|i| if let Item::Op(o) = i { Some(o) } else { None })
        .collect()
}

/// `src`'s ops, and its ops parsed again from how it prints, to `f`
fn twice(src: &str, f: impl FnOnce(&[Operation<'_>], &[Operation<'_>])) {
    let mut opt = MlirOpt::upstream();
    opt.load(AgxDialect);
    let module = opt.parse(src).unwrap();
    let printed = opt.show(&module).to_string();
    f(&ops(module), &ops(opt.parse(&printed).unwrap()));
}

/// `fixtures/usc.mlir` (Mesa's AGX2 disassembler cases decoded by Mesa's model) encodes as
/// `fixtures/usc.expected`, read again from its printed form too, each op its typed op
#[test]
fn usc_encodes_mesa_cases() {
    let src = include_str!("fixtures/usc.mlir");
    twice(src, |ops, reread| {
        let asm: Vec<&str> = src
            .lines()
            .skip(2)
            .filter_map(|l| l.strip_prefix("// "))
            .collect();
        let expected: Vec<&str> = include_str!("fixtures/usc.expected").lines().collect();
        assert_eq!(ops.len(), expected.len());
        let encode = |o: &Operation<'_>| match AgxIsa::encode(&o.name, &attrs(o)) {
            Ok(bytes) => bytes.iter().map(|x| format!("{x:02x}")).collect::<String>(),
            Err(e) => e.to_string(),
        };
        let mut bad = Vec::new();
        for (((op, re), want), asm) in ops.iter().zip(reread).zip(&expected).zip(&asm) {
            let (got, got_again) = (encode(op), encode(re));
            let typed = Usc::op(&op.name)
                .ok_or_else(|| "not a USC op".to_owned())
                .and_then(|o| o.verify(op).map_err(|e| e.to_string()));
            if got != *want || got_again != got || typed.is_err() {
                bad.push(format!("{asm}\n want {want}\n  got {got}\n printed and read again {got_again}\n typed {typed:?}"));
            }
        }
        assert!(
            bad.is_empty(),
            "{} of {} differ:\n{}",
            bad.len(),
            expected.len(),
            bad.join("\n")
        );
    });
}

/// `fixtures/oracle.mlir` (every struct, random field values) packs as Mesa's `agx_pack` packs the
/// same values (`fixtures/oracle.expected`)
#[test]
fn structs_pack_as_mesa() {
    twice(include_str!("fixtures/oracle.mlir"), |ops, reread| {
        let expected: Vec<&str> = include_str!("fixtures/oracle.expected").lines().collect();
        assert_eq!(ops.len(), expected.len());
        let pack = |o: &Operation<'_>| {
            let item = AGX.get(&o.name).unwrap();
            let dw = Pack(AGX)
                .pack(&o.name, &attrs(o))
                .unwrap_or_else(|e| panic!("{}: {e}", o.name));
            std::iter::once(item.name.to_owned())
                .chain(dw.iter().map(|d| format!("{d:08x}")))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut bad = Vec::new();
        for ((op, re), want) in ops.iter().zip(reread).zip(&expected) {
            let (got, got_again) = (pack(op), pack(re));
            let typed = AgxStructs::verify(op).map(|r| r.map_err(|e| e.to_string()));
            if got != *want || got_again != got || !matches!(typed, Some(Ok(()))) {
                bad.push(format!(
                    "want {want}\n got {got}\n printed and read again {got_again}\n typed {typed:?}"
                ));
            }
        }
        assert!(
            bad.is_empty(),
            "{} of {} differ:\n{}",
            bad.len(),
            expected.len(),
            bad.join("\n")
        );
    });
}
