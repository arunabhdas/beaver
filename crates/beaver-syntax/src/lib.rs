//! Beaver surface syntax: tokens, indentation-aware lexer, parser, AST and
//! the token-preserving formatter.

pub mod ast;
pub mod fmt;
pub mod lexer;
pub mod parser;
pub mod token;

use beaver_source::{Diagnostic, FileId, SourceMap};

/// Lex and parse one registered source file.
pub fn parse_file(sources: &SourceMap, file: FileId) -> Result<ast::Module, Vec<Diagnostic>> {
    let tokens = lexer::lex(file, sources.file(file))?;
    parser::parse(&tokens)
}
