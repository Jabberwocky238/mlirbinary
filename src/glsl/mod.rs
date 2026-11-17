mod build;
mod emit;
mod lex;
mod syntax;

use alloc::string::String;

use mlirformat::glsl::{GlslDialect, GlslStage};
use mlirformat::{Dialect, Error, MlirOpt, OpInfos, Operation};

use crate::{Pool, Read};
use emit::GlslReader;

/// GLSL (ES 1.00 / 3.00) source: read into mlirformat's `glsl` dialect (`glsl.shader`, its globals,
/// built-ins and texture ops, the rest as `func` / `arith` / `math` / `vector` / `scf` and SPIR-V
/// control flow and types)
pub struct GlslFormat {
    /// The stage the source is; when not given, a vertex shader if it writes `gl_Position`
    pub stage: Option<GlslStage>,
    opt: MlirOpt,
    infos: OpInfos,
}

impl GlslFormat {
    /// What a vertex shader writes
    const POSITION: &'static str = "gl_Position";
    /// What a shader's source starts with, comments and blank lines aside
    const VERSION: &'static str = "#version";

    pub fn new(stage: Option<GlslStage>) -> Self {
        let mut infos = OpInfos::upstream();
        for i in GlslDialect.op_infos() {
            infos.insert(i);
        }
        let mut opt = MlirOpt::upstream();
        opt.load(GlslDialect);
        Self { stage, opt, infos }
    }

    fn stage_of(&self, src: &str) -> GlslStage {
        self.stage.unwrap_or(if src.contains(Self::POSITION) { GlslStage::Vertex } else { GlslStage::Fragment })
    }

    /// The source with its comments and leading blank lines left out
    fn head(src: &str) -> &str {
        let mut rest = src.trim_start();
        loop {
            if let Some(r) = rest.strip_prefix("//") {
                rest = r.split_once('\n').map_or("", |(_, r)| r).trim_start();
            } else if let Some(r) = rest.strip_prefix("/*") {
                rest = r.split_once("*/").map_or("", |(_, r)| r).trim_start();
            } else {
                return rest;
            }
        }
    }
}

impl Default for GlslFormat {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Read for GlslFormat {
    fn name(&self) -> &'static str {
        "glsl"
    }

    fn recognizes(&self, bytes: &[u8]) -> bool {
        core::str::from_utf8(bytes).is_ok_and(|s| Self::head(s).starts_with(Self::VERSION))
    }

    fn read(&self, bytes: &[u8], _pool: &dyn Pool) -> Result<Operation<'static>, Error> {
        let src = core::str::from_utf8(bytes).map_err(|_| Error(String::from("glsl: the source is not UTF-8")))?;
        let (tree, op) = GlslReader::read(src, self.stage_of(src), &self.opt)?;
        Ok(tree.operation(op, &self.infos))
    }
}
