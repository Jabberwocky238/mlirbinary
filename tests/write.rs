use std::fs;
use std::path::{Path, PathBuf};

use mlirbinary::{Binaries, Format, SpirvFormat};
use mlirformat::{Item, MlirOpt, Operation};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spirv")
}

fn find<'o, 'a>(ops: &'o [Operation<'a>]) -> Option<&'o Operation<'a>> {
    ops.iter().find_map(|o| if o.name == "spirv.module" { Some(o) } else { o.regions.iter().flat_map(|r| &r.blocks).find_map(|b| find(&b.ops)) })
}

/// Every `.spv` here is what `mlir-translate --serialize-spirv` (23.1.5) wrote for the
/// `spirv.module` of the `.in.mlir` beside it.
#[test]
fn writes_as_mlir_translate() {
    let opt = MlirOpt::upstream();
    let filter = std::env::var("FILTER").ok();
    let mut stems: Vec<String> = fs::read_dir(dir()).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".spv").map(str::to_owned)).collect();
    stems.sort();
    let binaries = Binaries::builtin();
    let (mut exact, mut failed) = (0, Vec::new());
    for stem in stems.iter().filter(|s| filter.as_deref().is_none_or(|f| s.contains(f))) {
        let want = fs::read(dir().join(format!("{stem}.spv"))).unwrap();
        let text = fs::read_to_string(dir().join(format!("{stem}.in.mlir"))).unwrap();
        let got = opt.parse(&text).map_err(|e| e.to_string()).and_then(|m| {
            let ops: Vec<Operation<'_>> = m.items.into_iter().filter_map(|i| if let Item::Op(o) = i { Some(o) } else { None }).collect();
            let module = find(&ops).ok_or("no spirv.module")?;
            binaries.write("spirv", module).map_err(|e| e.0)
        });
        match got {
            Ok(got) if got == want => exact += 1,
            Ok(got) => {
                if std::env::var_os("SHOW").is_some() {
                    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/spirv-got");
                    fs::create_dir_all(&out).unwrap();
                    fs::write(out.join(format!("{stem}.spv")), &got).unwrap();
                }
                let word = |b: &[u8], i: usize| b.get(i * 4..i * 4 + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap()));
                let at = (0..).find(|i| word(&got, *i) != word(&want, *i)).unwrap();
                failed.push(format!("{stem}: word {at}: mlir-translate {:08x?}, mlirbinary {:08x?} ({} vs {} words)", word(&want, at), word(&got, at), want.len() / 4, got.len() / 4));
            }
            Err(e) => failed.push(format!("{stem}: {e}")),
        }
    }
    eprintln!("{exact} of {} write byte for byte", exact + failed.len());
    /// The modules written byte for byte so far; the others use ops and decorations not written yet
    const FLOOR: usize = 203;
    assert!(filter.is_some() || exact >= FLOOR, "{exact} written byte for byte, fewer than {FLOOR}");
    for f in &failed {
        eprintln!("  {f}");
    }
    assert!(SpirvFormat::default().recognizes(&fs::read(dir().join(format!("{}.spv", stems[0]))).unwrap()));
}
