//! Source files, byte spans and structured diagnostics for the Beaver compiler.
//!
//! Every compiler stage reports problems as [`Diagnostic`] values carrying a
//! stable error code (`B1xxx` .. `B6xxx`) and a primary [`Span`]. Rendering to
//! the human-readable `error[B3002]: ... --> file:line:col` form happens here so
//! that all stages share one format.

use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

/// A half-open byte range `[start, end)` inside one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, start: usize, end: usize) -> Span {
        Span {
            file,
            start: start as u32,
            end: end.max(start) as u32,
        }
    }

    /// Smallest span covering both `self` and `other` (same file assumed).
    pub fn to(self, other: Span) -> Span {
        Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

pub struct SourceFile {
    /// Path shown to users, e.g. `src/main.bv` or `mathkit-1.0.0/src/lib.bv`.
    pub display: String,
    /// Real file-system path (may be empty for in-memory sources).
    pub path: PathBuf,
    /// LF-normalized UTF-8 text.
    pub text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// One-based line and column (column counted in Unicode scalar values).
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line];
        let col = self.text[start..floor_char_boundary(&self.text, offset)]
            .chars()
            .count();
        (line + 1, col + 1)
    }

    pub fn line_text(&self, line: usize) -> &str {
        let start = self.line_starts[line - 1];
        let end = self
            .line_starts
            .get(line)
            .map(|e| e - 1)
            .unwrap_or(self.text.len());
        &self.text[start..end.max(start)]
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap::default()
    }

    /// Register already-decoded text. CRLF is normalized to LF.
    pub fn add(&mut self, display: impl Into<String>, path: PathBuf, text: &str) -> FileId {
        let text = text.replace("\r\n", "\n");
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile {
            display: display.into(),
            path,
            text,
            line_starts,
        });
        id
    }

    /// Decode raw bytes as UTF-8, returning a B1001 diagnostic on failure.
    pub fn add_bytes(
        &mut self,
        display: impl Into<String>,
        path: PathBuf,
        bytes: &[u8],
    ) -> Result<FileId, Diagnostic> {
        let display = display.into();
        match std::str::from_utf8(bytes) {
            Ok(text) => Ok(self.add(display, path, text)),
            Err(e) => {
                let valid = e.valid_up_to();
                // Register the valid prefix so the diagnostic has a location.
                let prefix = String::from_utf8_lossy(&bytes[..valid]).into_owned();
                let id = self.add(display, path, &prefix);
                let len = self.file(id).text.len();
                Err(Diagnostic::error(
                    "B1001",
                    format!("source file is not valid UTF-8 (invalid byte at offset {valid})"),
                    Span::new(id, len, len),
                ))
            }
        }
    }

    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn location(&self, span: Span) -> (String, usize, usize) {
        let f = self.file(span.file);
        let (l, c) = f.line_col(span.start as usize);
        (f.display.clone(), l, c)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Option<Span>,
    pub label: Option<String>,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>, span: Span) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            span: Some(span),
            label: None,
            notes: Vec::new(),
            help: None,
        }
    }

    /// A diagnostic not tied to source text (package/toolchain errors).
    pub fn global(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            span: None,
            label: None,
            notes: Vec::new(),
            help: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Diagnostic {
        self.label = Some(label.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Diagnostic {
        self.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Diagnostic {
        self.help = Some(help.into());
        self
    }

    /// Render in the documented multi-line format.
    pub fn render(&self, sources: Option<&SourceMap>) -> String {
        let mut out = String::new();
        let kind = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let _ = writeln!(out, "{kind}[{}]: {}", self.code, self.message);
        if let (Some(span), Some(sm)) = (self.span, sources) {
            let file = sm.file(span.file);
            let (line, col) = file.line_col(span.start as usize);
            let text = file.line_text(line);
            let gutter = line.to_string().len();
            let pad = " ".repeat(gutter);
            let _ = writeln!(out, "{pad}--> {}:{line}:{col}", file.display);
            let _ = writeln!(out, "{pad} |");
            let _ = writeln!(out, "{line} | {text}");
            let (end_line, end_col) = file.line_col(span.end as usize);
            let width = if end_line == line && end_col > col {
                end_col - col
            } else {
                1
            };
            let label = self.label.as_deref().unwrap_or("");
            let _ = writeln!(
                out,
                "{pad} | {}{} {label}",
                " ".repeat(col - 1),
                "^".repeat(width)
            );
            let _ = writeln!(out, "{pad} |");
        }
        for note in &self.notes {
            let _ = writeln!(out, "note: {note}");
        }
        if let Some(help) = &self.help {
            let _ = writeln!(out, "help: {help}");
        }
        out
    }
}

pub type Diagnostics = Vec<Diagnostic>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_column_are_one_based() {
        let mut sm = SourceMap::new();
        let id = sm.add("a.bv", PathBuf::new(), "ab\ncd\n");
        let f = sm.file(id);
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_col(3), (2, 1));
        assert_eq!(f.line_col(4), (2, 2));
    }

    #[test]
    fn crlf_is_normalized() {
        let mut sm = SourceMap::new();
        let id = sm.add("a.bv", PathBuf::new(), "a\r\nb\r\n");
        assert_eq!(sm.file(id).text, "a\nb\n");
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let mut sm = SourceMap::new();
        let id = sm.add("a.bv", PathBuf::new(), "\"é\" x");
        // `x` is the 5th character but the 6th byte.
        assert_eq!(sm.file(id).line_col(5), (1, 5));
    }

    #[test]
    fn invalid_utf8_reports_b1001() {
        let mut sm = SourceMap::new();
        let err = sm
            .add_bytes("bad.bv", PathBuf::new(), b"fn main():\n\xff\n")
            .unwrap_err();
        assert_eq!(err.code, "B1001");
        let (_, line, _) = sm.location(err.span.unwrap());
        assert_eq!(line, 2);
    }

    #[test]
    fn render_contains_code_location_and_caret() {
        let mut sm = SourceMap::new();
        let id = sm.add("src/main.bv", PathBuf::new(), "let x = 1\nlet y = edit x\n");
        let d = Diagnostic::error("B3002", "cannot edit `x`", Span::new(id, 18, 22))
            .with_label("conflicting borrow")
            .with_help("use scope:");
        let r = d.render(Some(&sm));
        assert!(r.starts_with("error[B3002]: cannot edit `x`"));
        assert!(r.contains("--> src/main.bv:2:9"));
        assert!(r.contains("^^^^ conflicting borrow"));
        assert!(r.contains("help: use scope:"));
    }
}
