use std::fs;
use std::path::{Path, PathBuf};

use mlirbinary::{Binaries, Format, SpirvFormat};
use mlirformat::{Item, MlirOpt, Module};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spirv")
}

/// Generic text with SSA names and block labels renumbered in order of appearance
struct Canon;

impl Canon {
    fn of(module: &Module<'_>) -> String {
        let text = MlirOpt::default().show(module).to_string();
        let mut names: Vec<String> = Vec::new();
        let mut out = String::new();
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i] as char;
            if text[i..].starts_with("\"spirv.func\"") {
                names.clear();
            }
            let sigil = c == '%';
            if sigil && !(i > 0 && (b[i - 1] as char).is_ascii_alphanumeric()) {
                let mut j = i + 1;
                while j < b.len() && ((b[j] as char).is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'.' || b[j] == b'$' || b[j] == b'-') {
                    j += 1;
                }
                let tok = String::from(&text[i..j]);
                let k = names.iter().position(|n| *n == tok).unwrap_or_else(|| {
                    names.push(tok.clone());
                    names.len() - 1
                });
                out.push_str(&format!("{c}{k}"));
                i = j;
            } else {
                out.push(c);
                i += 1;
            }
        }
        out
    }
}

/// Every `.read.mlir` here is what `mlir-translate --deserialize-spirv` (23.1.5) made of the `.spv`
/// beside it.
#[test]
fn reads_as_mlir_translate() {
    let opt = MlirOpt::upstream();
    let filter = std::env::var("FILTER").ok();
    let show = std::env::var_os("SHOW").is_some();
    let mut stems: Vec<String> = fs::read_dir(dir()).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".spv").map(str::to_owned)).collect();
    stems.sort();
    let binaries = Binaries::builtin();
    let (mut exact, mut failed) = (0, Vec::new());
    for stem in stems.iter().filter(|s| filter.as_deref().is_none_or(|f| s.contains(f))) {
        let bytes = fs::read(dir().join(format!("{stem}.spv"))).unwrap();
        assert!(SpirvFormat::default().recognizes(&bytes));
        let text = fs::read_to_string(dir().join(format!("{stem}.read.mlir"))).unwrap();
        let want = Canon::of(&opt.parse(&text).unwrap());
        let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| binaries.read(&bytes))).map_err(|_| String::from("panicked")).and_then(|r| r.map_err(|e| e.0)).map(|op| Canon::of(&Module { items: vec![Item::Op(op)] }));
        match got {
            Ok(got) if got == want => exact += 1,
            Ok(got) => {
                let line = got.lines().zip(want.lines()).position(|(a, b)| a != b).unwrap_or(0);
                if show {
                    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/spirv-read");
                    fs::create_dir_all(&out).unwrap();
                    fs::write(out.join(format!("{stem}.got.mlir")), &got).unwrap();
                    fs::write(out.join(format!("{stem}.want.mlir")), &want).unwrap();
                }
                failed.push(format!("{stem}: line {}: mlirbinary `{}` / mlir-translate `{}`", line + 1, got.lines().nth(line).unwrap_or("").trim(), want.lines().nth(line).unwrap_or("").trim()));
            }
            Err(e) => failed.push(format!("{stem}: {e}")),
        }
    }
    eprintln!("{exact} of {} read as mlir-translate does", exact + failed.len());
    for f in &failed {
        eprintln!("  {}", &f[..f.len().min(300)]);
    }
    assert!(failed.is_empty() || filter.is_some(), "{} of {} differ from mlir-translate", failed.len(), exact + failed.len());
}
