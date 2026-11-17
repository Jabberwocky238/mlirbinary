use std::fs;
use std::path::{Path, PathBuf};

use mlirbinary::{Binaries, BytecodeFormat, Format};
use mlirformat::{Item, MlirOpt};

fn dir() -> PathBuf {
    std::env::var_os("MLIRBC_DIR").map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mlirbc"), PathBuf::from)
}

/// Every `.mlirbc` here is what `mlir-opt --emit-bytecode` (23.1.5) wrote for the `.in.mlir`
/// beside it (generic form, every location spelled out).
#[test]
fn writes_as_mlir_opt() {
    let opt = MlirOpt::upstream();
    let filter = std::env::var("FILTER").ok();
    let mut stems: Vec<String> = fs::read_dir(dir()).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".mlirbc").map(str::to_owned)).collect();
    stems.sort();
    let binaries = Binaries::builtin();
    let (mut exact, mut failed) = (0, Vec::new());
    for stem in stems.iter().filter(|s| filter.as_deref().is_none_or(|f| s.contains(f))) {
        let want = fs::read(dir().join(format!("{stem}.mlirbc"))).unwrap();
        let text = fs::read_to_string(dir().join(format!("{stem}.in.mlir"))).unwrap();
        let got = opt.parse(&text).map_err(|e| e.to_string()).and_then(|m| {
            let op = m.items.into_iter().find_map(|i| if let Item::Op(o) = i { Some(o) } else { None }).ok_or("no op")?;
            binaries.write("mlirbc", &op).map_err(|e| e.0)
        });
        match got {
            Ok(got) if got == want => exact += 1,
            Ok(got) => {
                if std::env::var_os("SHOW").is_some() {
                    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/mlirbc-got");
                    fs::create_dir_all(&out).unwrap();
                    fs::write(out.join(format!("{stem}.mlirbc")), &got).unwrap();
                }
                let at = (0..).find(|i| got.get(*i) != want.get(*i)).unwrap();
                failed.push(format!("{stem}: byte {at}: mlir-opt {:02x?}, mlirbinary {:02x?} ({} vs {} bytes)", want.get(at), got.get(at), want.len(), got.len()));
            }
            Err(e) => failed.push(format!("{stem}: {e}")),
        }
    }
    eprintln!("{exact} of {} write byte for byte", exact + failed.len());
    /// Modules written byte for byte; the rest use the llvm dialect's own encodings, floats of
    /// fewer than eight bits or `f8E8M0FNU`, resources, or string elements of a dialect type
    const FLOOR: usize = 2240;
    assert!(filter.is_some() || std::env::var_os("MLIRBC_DIR").is_some() || exact >= FLOOR, "{exact} written byte for byte, fewer than {FLOOR}");
    for f in &failed {
        eprintln!("  {f}");
    }
    assert!(BytecodeFormat::default().recognizes(&fs::read(dir().join(format!("{}.mlirbc", stems[0]))).unwrap()));
}
