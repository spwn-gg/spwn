//! TypeScript workflows: types are stripped before the module is loaded. There is no
//! type checking at run time — that's the editor's job, with `spwn.d.ts`.

use oxc::allocator::Allocator;
use oxc::codegen::Codegen;
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};
use std::path::Path;

pub(crate) fn strip_types(source: &str, path: &Path) -> Result<String, String> {
    // This runs inside QuickJS's module-loader callback, where a panic can't unwind and
    // would abort all of spwn — so a compiler crash becomes this file's load error.
    std::panic::catch_unwind(|| strip(source, path)).unwrap_or_else(|_| {
        Err(format!(
            "{}: the TypeScript compiler crashed on this file",
            path.display()
        ))
    })
}

fn strip(source: &str, path: &Path) -> Result<String, String> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .with_module(true);
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(describe(path, parsed.diagnostics.iter().map(ToString::to_string)));
    }
    let mut program = parsed.program;
    // Enum evaluation is required for the transformer to lower `enum`s.
    let scoping = SemanticBuilder::new()
        .with_enum_eval(true)
        .build(&program)
        .semantic
        .into_scoping();
    let transformed = Transformer::new(&allocator, path, &TransformOptions::default())
        .build_with_scoping(scoping, &mut program);
    if !transformed.diagnostics.is_empty() {
        return Err(describe(path, transformed.diagnostics.iter().map(ToString::to_string)));
    }
    Ok(Codegen::new().build(&program).code)
}

fn describe(path: &Path, errors: impl Iterator<Item = String>) -> String {
    format!("{}: {}", path.display(), errors.collect::<Vec<_>>().join("; "))
}
