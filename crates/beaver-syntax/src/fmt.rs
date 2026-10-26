//! Canonical formatter (`beaver fmt`).
//!
//! Beaver has no multi-line tokens or line continuations, so formatting works
//! on physical lines: indentation is normalized to 4 spaces per level, spacing
//! between tokens is canonicalized, comments are preserved, and literal tokens
//! (strings, numbers, identifiers) are copied byte-for-byte from the source.
//! The output is re-lexed and compared token-by-token with the input; if the
//! token streams differ the formatter refuses to produce output.

use crate::lexer::{lex, lex_line};
use crate::token::{Tok, Token};
use beaver_source::{Diagnostic, FileId, SourceFile, SourceMap, Span};
use std::path::PathBuf;

enum Line {
    Blank,
    Comment {
        col: usize,
        text: String,
    },
    Code {
        level: usize,
        text: String,
        ends_colon: bool,
    },
}

pub fn format_source(file: FileId, src: &SourceFile) -> Result<String, Vec<Diagnostic>> {
    // Refuse to format code that does not parse.
    let original_tokens = lex(file, src)?;
    crate::parser::parse(&original_tokens)?;

    let text = src.text.as_str();
    let mut lines = Vec::new();
    let mut snapshots: Vec<Vec<usize>> = Vec::new(); // stack after each code line
    let mut stack: Vec<usize> = vec![0];
    let mut errors = Vec::new();
    let mut start = 0;
    while start <= text.len() {
        let end = text[start..]
            .find('\n')
            .map(|i| start + i)
            .unwrap_or(text.len());
        let line = &text[start..end];
        let width = line
            .bytes()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count();
        let rest = line[width..].trim_end();
        if rest.is_empty() {
            lines.push(Line::Blank);
        } else if rest.starts_with('#') {
            lines.push(Line::Comment {
                col: width,
                text: rest.to_string(),
            });
        } else {
            if width > *stack.last().unwrap() {
                stack.push(width);
            } else {
                while *stack.last().unwrap() > width {
                    stack.pop();
                }
            }
            let level = stack.len() - 1;
            let lt = lex_line(file, text, start + width, end, &mut errors);
            let mut out = render_tokens(text, &lt.tokens);
            if let Some(c) = lt.comment {
                out.push_str("  ");
                out.push_str(text[c..end].trim_end());
            }
            let ends_colon = matches!(
                lt.tokens.last(),
                Some(Token {
                    tok: Tok::Colon,
                    ..
                })
            );
            lines.push(Line::Code {
                level,
                text: out,
                ends_colon,
            });
            snapshots.push(stack.clone());
        }
        if end == text.len() {
            break;
        }
        start = end + 1;
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // Resolve comment indentation levels.
    let mut out = String::new();
    let mut code_seen = 0usize;
    let mut pending_blank = 0usize;
    let mut emitted_any = false;
    for i in 0..lines.len() {
        match &lines[i] {
            Line::Blank => {
                pending_blank += 1;
                continue;
            }
            Line::Comment { .. } | Line::Code { .. } => {}
        }
        if emitted_any {
            for _ in 0..pending_blank.min(2) {
                out.push('\n');
            }
        }
        pending_blank = 0;
        emitted_any = true;
        match &lines[i] {
            Line::Code { level, text, .. } => {
                out.push_str(&"    ".repeat(*level));
                out.push_str(text);
                out.push('\n');
                code_seen += 1;
            }
            Line::Comment { col, text } => {
                let prev_stack: &[usize] = if code_seen == 0 {
                    &[0]
                } else {
                    &snapshots[code_seen - 1]
                };
                let prev_colon = if code_seen == 0 {
                    false
                } else {
                    lines[..i]
                        .iter()
                        .rev()
                        .find_map(|l| match l {
                            Line::Code { ends_colon, .. } => Some(*ends_colon),
                            _ => None,
                        })
                        .unwrap_or(false)
                };
                let next_level = lines[i + 1..]
                    .iter()
                    .find_map(|l| match l {
                        Line::Code { level, .. } => Some(*level),
                        _ => None,
                    })
                    .unwrap_or(0);
                let depth = prev_stack.len() - 1;
                let level = if prev_colon || next_level > depth {
                    next_level
                } else {
                    (next_level..=depth)
                        .rev()
                        .find(|d| prev_stack[*d] <= *col)
                        .unwrap_or(next_level)
                };
                out.push_str(&"    ".repeat(level));
                out.push_str(text);
                out.push('\n');
            }
            Line::Blank => unreachable!(),
        }
    }

    // Safety net: the formatted text must lex to the identical token stream.
    let mut sm = SourceMap::new();
    let id = sm.add("<formatted>", PathBuf::new(), &out);
    let new_tokens = lex(id, sm.file(id)).map_err(|_| {
        vec![Diagnostic::error(
            "B1099",
            "internal formatter error: output does not lex",
            Span::new(file, 0, 0),
        )]
    })?;
    let a: Vec<&Tok> = original_tokens.iter().map(|t| &t.tok).collect();
    let b: Vec<&Tok> = new_tokens.iter().map(|t| &t.tok).collect();
    if a != b {
        return Err(vec![Diagnostic::error(
            "B1099",
            "internal formatter error: formatting would change the program; file left unchanged",
            Span::new(file, 0, 0),
        )]);
    }
    Ok(out)
}

fn is_operator(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Plus
            | Tok::Minus
            | Tok::Star
            | Tok::Slash
            | Tok::Percent
            | Tok::EqEq
            | Tok::NotEq
            | Tok::Lt
            | Tok::Le
            | Tok::Gt
            | Tok::Ge
            | Tok::Assign
            | Tok::PlusAssign
            | Tok::MinusAssign
            | Tok::StarAssign
            | Tok::SlashAssign
            | Tok::Arrow
    )
}

fn render_tokens(text: &str, tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    let mut prev: Option<&Tok> = None;
    let mut prev_unary = false;
    for t in tokens {
        let cur = &t.tok;
        let unary_minus = *cur == Tok::Minus
            && match prev {
                None => true,
                Some(p) => {
                    is_operator(p)
                        || matches!(p, Tok::LParen | Tok::LBracket | Tok::Comma | Tok::Colon)
                        || (p.is_keyword() && !matches!(p, Tok::True | Tok::False))
                }
            };
        if let Some(p) = prev {
            let glued = prev_unary
                || matches!(cur, Tok::RParen | Tok::RBracket | Tok::Comma | Tok::Colon)
                || *cur == Tok::Dot
                || *p == Tok::Dot
                || matches!(p, Tok::LParen | Tok::LBracket)
                // named arguments: `Point(x=1)`
                || (depth > 0 && (*cur == Tok::Assign || *p == Tok::Assign));
            let space = if glued {
                false
            } else if matches!(cur, Tok::LParen | Tok::LBracket) {
                // calls and indexing attach to their callee
                !matches!(p, Tok::Ident(_) | Tok::RParen | Tok::RBracket | Tok::Str(_))
            } else {
                true
            };
            if space {
                out.push(' ');
            }
        }
        match cur {
            Tok::Ident(_) | Tok::Int(_) | Tok::Float(_) | Tok::Str(_) => {
                out.push_str(&text[t.span.start as usize..t.span.end as usize]);
            }
            other => out.push_str(other.text()),
        }
        match cur {
            Tok::LParen | Tok::LBracket => depth += 1,
            Tok::RParen | Tok::RBracket => depth -= 1,
            _ => {}
        }
        prev_unary = unary_minus;
        prev = Some(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        let mut sm = SourceMap::new();
        let id = sm.add("t.bv", PathBuf::new(), src);
        format_source(id, sm.file(id)).unwrap()
    }

    #[test]
    fn normalizes_indentation_and_spacing() {
        let src = "fn  add( a:Int,b :Int )->Int:\n        return a+b*-c\n";
        assert_eq!(
            fmt(src),
            "fn add(a: Int, b: Int) -> Int:\n    return a + b * -c\n"
        );
    }

    #[test]
    fn named_args_and_indexing() {
        let src = "fn f():\n    let p = Point( x = 1 , y=2 )\n    print(xs [ 0 ])\n    let l = [ 1, 2 ]\n";
        assert_eq!(
            fmt(src),
            "fn f():\n    let p = Point(x=1, y=2)\n    print(xs[0])\n    let l = [1, 2]\n"
        );
    }

    #[test]
    fn preserves_strings_and_comments() {
        let src = "# header\nfn f():\n    # inside\n    print(\"a  ,b\\n\")   # trailing  \n";
        assert_eq!(
            fmt(src),
            "# header\nfn f():\n    # inside\n    print(\"a  ,b\\n\")  # trailing\n"
        );
    }

    #[test]
    fn idempotent() {
        let src = "use util\n\n\n\n\nfn main():\n        var x = 0\n        while x<3 :\n                x += 1\n                # done?\n        print( x )\n# end\n";
        let once = fmt(src);
        let twice = fmt(&once);
        assert_eq!(once, twice);
        assert!(!once.contains("\n\n\n\n"));
    }

    #[test]
    fn comment_at_block_end_keeps_its_level() {
        let src = "fn f():\n    if a:\n        pass\n        # still inside\n    pass\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn unary_after_keyword() {
        assert_eq!(
            fmt("fn f() -> Int:\n    return - 1\n"),
            "fn f() -> Int:\n    return -1\n"
        );
        assert_eq!(
            fmt("fn f():\n    g(not  a, view  b)\n"),
            "fn f():\n    g(not a, view b)\n"
        );
    }

    #[test]
    fn refuses_unparseable_input() {
        let mut sm = SourceMap::new();
        let id = sm.add("t.bv", PathBuf::new(), "fn f(:\n");
        assert!(format_source(id, sm.file(id)).is_err());
    }
}
