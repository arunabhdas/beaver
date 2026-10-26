//! Semantic analysis for Beaver: module loading and import resolution,
//! type checking, and ownership/borrow checking.
//!
//! [`analyze`] is the only way to obtain a [`hir::Program`]; it returns one
//! only when every stage succeeded, so later stages (MIR, C backend) can rely
//! on the program being type- and ownership-correct.

pub mod hir;
pub mod load;
pub mod ownership;
pub mod typeck;
pub mod types;

pub use load::{PackageInput, ProgramInput, ProgramKind};

use beaver_source::{Diagnostic, SourceMap};

/// Diagnostics from a failed analysis, with the sources needed to render them.
pub struct Failure {
    pub sources: SourceMap,
    pub diagnostics: Vec<Diagnostic>,
}

impl Failure {
    pub fn render(&self) -> String {
        self.diagnostics
            .iter()
            .map(|d| d.render(Some(&self.sources)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn analyze(input: &ProgramInput) -> Result<hir::Program, Failure> {
    let loaded = match load::load(input) {
        Ok(l) => l,
        Err((sources, diagnostics)) => {
            return Err(Failure {
                sources,
                diagnostics,
            })
        }
    };
    let typed = match typeck::check(&loaded, input) {
        Ok(t) => t,
        Err(diagnostics) => {
            return Err(Failure {
                sources: loaded.sources,
                diagnostics,
            })
        }
    };
    let errors = ownership::check(&typed.functions, &typed.records, &loaded.sources);
    if !errors.is_empty() {
        return Err(Failure {
            sources: loaded.sources,
            diagnostics: errors,
        });
    }
    Ok(hir::Program {
        sources: loaded.sources,
        records: typed.records,
        functions: typed.functions,
        entry: typed.entry,
        tests: typed.tests,
        package_name: input.packages[input.root].name.clone(),
    })
}
