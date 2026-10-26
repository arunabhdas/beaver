//! Indentation-aware lexer.
//!
//! Beaver has no line continuation and no multi-line tokens, so every
//! physical line that is not blank/comment-only is exactly one logical line.
//! The lexer handles leading indentation per line (emitting INDENT / DEDENT),
//! then tokenizes the rest of the line with [`lex_line`], which the formatter
//! reuses.

use crate::token::{Tok, Token};
use beaver_source::{Diagnostic, FileId, SourceFile, Span};

/// Result of tokenizing the content of a single physical line.
pub struct LineTokens {
    pub tokens: Vec<Token>,
    /// Byte offset of a trailing `#` comment, if any.
    pub comment: Option<usize>,
}

pub fn lex(file: FileId, src: &SourceFile) -> Result<Vec<Token>, Vec<Diagnostic>> {
    let text = src.text.as_str();
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    let mut stack: Vec<usize> = vec![0];
    let mut line_start = 0usize;
    while line_start < text.len() {
        let line_end = text[line_start..]
            .find('\n')
            .map(|i| line_start + i)
            .unwrap_or(text.len());
        let line = &text[line_start..line_end];
        // Leading whitespace analysis.
        let mut width = 0usize;
        let mut has_tab = false;
        for b in line.bytes() {
            match b {
                b' ' => width += 1,
                b'\t' => {
                    has_tab = true;
                    width += 1;
                }
                _ => break,
            }
        }
        let rest = &line[width..];
        let blank =
            rest.is_empty() || rest.starts_with('#') || rest.trim_matches([' ', '\t']).is_empty();
        if !blank {
            let indent_span = Span::new(file, line_start, line_start + width);
            if has_tab {
                errors.push(
                    Diagnostic::error("B1002", "tab character in indentation", indent_span)
                        .with_help("indent with spaces only (4 per level)"),
                );
            } else if width % 4 != 0 {
                errors.push(
                    Diagnostic::error(
                        "B1003",
                        format!("indentation of {width} spaces is not a multiple of 4"),
                        indent_span,
                    )
                    .with_help("indent each block level with 4 spaces; `beaver fmt` normalizes indentation"),
                );
            } else {
                let top = *stack.last().unwrap();
                let at = Span::new(file, line_start + width, line_start + width);
                if width > top {
                    stack.push(width);
                    tokens.push(Token {
                        tok: Tok::Indent,
                        span: at,
                    });
                } else if width < top {
                    while *stack.last().unwrap() > width {
                        stack.pop();
                        tokens.push(Token {
                            tok: Tok::Dedent,
                            span: at,
                        });
                    }
                    if *stack.last().unwrap() != width {
                        errors.push(
                            Diagnostic::error(
                                "B1004",
                                "dedent does not match any enclosing indentation level",
                                indent_span,
                            )
                            .with_help("align this line with an enclosing block"),
                        );
                        // Recover by treating it as the enclosing level.
                        stack.push(width);
                    }
                }
            }
            let lt = lex_line(file, text, line_start + width, line_end, &mut errors);
            if !lt.tokens.is_empty() {
                tokens.extend(lt.tokens);
                tokens.push(Token {
                    tok: Tok::Newline,
                    span: Span::new(file, line_end, line_end),
                });
            }
        } else if rest.contains('\r') {
            // Lone carriage returns in otherwise blank lines.
            let pos = line_start + line.find('\r').unwrap();
            errors.push(Diagnostic::error(
                "B1016",
                "stray carriage return (only LF or CRLF line endings are allowed)",
                Span::new(file, pos, pos + 1),
            ));
        }
        line_start = line_end + 1;
    }
    let end = Span::new(file, text.len(), text.len());
    while stack.len() > 1 {
        stack.pop();
        tokens.push(Token {
            tok: Tok::Dedent,
            span: end,
        });
    }
    tokens.push(Token {
        tok: Tok::Eof,
        span: end,
    });
    if errors.is_empty() {
        Ok(tokens)
    } else {
        Err(errors)
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Tokenize `text[start..end]` (one physical line without its newline and
/// without leading indentation).
pub fn lex_line(
    file: FileId,
    text: &str,
    start: usize,
    end: usize,
    errors: &mut Vec<Diagnostic>,
) -> LineTokens {
    let mut tokens = Vec::new();
    let bytes = text.as_bytes();
    let mut i = start;
    let mut comment = None;
    while i < end {
        let c = text[i..].chars().next().unwrap();
        if c == ' ' || c == '\t' {
            i += 1;
            continue;
        }
        if c == '#' {
            comment = Some(i);
            break;
        }
        let tok_start = i;
        let push = |tokens: &mut Vec<Token>, tok: Tok, len: usize| {
            tokens.push(Token {
                tok,
                span: Span::new(file, tok_start, tok_start + len),
            });
        };
        if is_ident_start(c) {
            let mut j = i;
            while j < end && is_ident_continue(bytes[j] as char) && bytes[j].is_ascii() {
                j += 1;
            }
            let word = &text[i..j];
            let tok = Tok::keyword(word).unwrap_or_else(|| Tok::Ident(word.to_string()));
            push(&mut tokens, tok, j - i);
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < end && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let mut is_float = false;
            if j + 1 < end && bytes[j] == b'.' && bytes[j + 1].is_ascii_digit() {
                is_float = true;
                j += 1;
                while j < end && bytes[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if j < end
                && (is_ident_continue(bytes[j] as char) || bytes[j] == b'.')
                && !(bytes[j] == b'.' && !is_float)
            {
                // e.g. `12abc` or `1.5.3`
                let mut k = j;
                while k < end && (is_ident_continue(bytes[k] as char) || bytes[k] == b'.') {
                    k += 1;
                }
                errors.push(Diagnostic::error(
                    "B1011",
                    format!("invalid numeric literal `{}`", &text[i..k]),
                    Span::new(file, i, k),
                ));
                i = k;
                continue;
            }
            let lit = &text[i..j];
            let span = Span::new(file, i, j);
            if is_float {
                match lit.parse::<f64>() {
                    Ok(v) if v.is_finite() => tokens.push(Token {
                        tok: Tok::Float(v),
                        span,
                    }),
                    _ => errors.push(Diagnostic::error(
                        "B1011",
                        format!("float literal `{lit}` is out of range"),
                        span,
                    )),
                }
            } else {
                match lit.parse::<i64>() {
                    Ok(v) => tokens.push(Token {
                        tok: Tok::Int(v),
                        span,
                    }),
                    Err(_) => errors.push(
                        Diagnostic::error(
                            "B1010",
                            format!("integer literal `{lit}` does not fit in a 64-bit signed Int"),
                            span,
                        )
                        .with_note("the largest Int literal is 9223372036854775807")
                        .with_help("write the minimum as `-9223372036854775807 - 1`"),
                    ),
                }
            }
            i = j;
            continue;
        }
        if c == '"' {
            match lex_string(file, text, i, end) {
                Ok((value, j)) => {
                    tokens.push(Token {
                        tok: Tok::Str(value),
                        span: Span::new(file, i, j),
                    });
                    i = j;
                }
                Err((d, j)) => {
                    errors.push(d);
                    i = j;
                }
            }
            continue;
        }
        let next = if i + 1 < end { bytes[i + 1] } else { 0 };
        let two = |a: Tok, b: Tok| if next == b'=' { (a, 2) } else { (b, 1) };
        let (tok, len) = match c {
            '(' => (Tok::LParen, 1),
            ')' => (Tok::RParen, 1),
            '[' => (Tok::LBracket, 1),
            ']' => (Tok::RBracket, 1),
            ',' => (Tok::Comma, 1),
            ':' => (Tok::Colon, 1),
            '.' => (Tok::Dot, 1),
            '%' => (Tok::Percent, 1),
            '+' => two(Tok::PlusAssign, Tok::Plus),
            '*' => two(Tok::StarAssign, Tok::Star),
            '/' => two(Tok::SlashAssign, Tok::Slash),
            '=' => two(Tok::EqEq, Tok::Assign),
            '<' => two(Tok::Le, Tok::Lt),
            '>' => two(Tok::Ge, Tok::Gt),
            '-' => {
                if next == b'>' {
                    (Tok::Arrow, 2)
                } else {
                    two(Tok::MinusAssign, Tok::Minus)
                }
            }
            '!' if next == b'=' => (Tok::NotEq, 2),
            _ => {
                let len = c.len_utf8();
                let span = Span::new(file, i, i + len);
                let d = match c {
                    '&' => Diagnostic::error("B1009", "`&` is not a Beaver operator", span)
                        .with_help("borrow with `view value` or `edit value`; use `and` for logic"),
                    '!' => Diagnostic::error("B1009", "`!` is not a Beaver operator", span)
                        .with_help("use `not` for Boolean negation"),
                    '{' | '}' | ';' => Diagnostic::error(
                        "B1009",
                        format!("unexpected `{c}`: Beaver blocks use `:` and indentation"),
                        span,
                    ),
                    '|' | '^' | '~' => Diagnostic::error(
                        "B1009",
                        format!("`{c}`: bitwise operators are not available in Beaver 0.1"),
                        span,
                    )
                    .with_help("use `or` / `and` for Boolean logic"),
                    '\r' => Diagnostic::error(
                        "B1016",
                        "stray carriage return (only LF or CRLF line endings are allowed)",
                        span,
                    ),
                    _ => Diagnostic::error(
                        "B1009",
                        format!("unexpected character `{}`", c.escape_default()),
                        span,
                    ),
                };
                errors.push(d);
                i += len;
                continue;
            }
        };
        push(&mut tokens, tok, len);
        i += len;
    }
    LineTokens { tokens, comment }
}

fn lex_string(
    file: FileId,
    text: &str,
    start: usize,
    end: usize,
) -> Result<(String, usize), (Diagnostic, usize)> {
    let mut value = String::new();
    let mut i = start + 1;
    loop {
        if i >= end {
            return Err((
                Diagnostic::error(
                    "B1007",
                    "unterminated string literal",
                    Span::new(file, start, end),
                )
                .with_note("Beaver strings cannot span multiple lines"),
                end,
            ));
        }
        let c = text[i..].chars().next().unwrap();
        match c {
            '"' => return Ok((value, i + 1)),
            '\\' => {
                let esc_start = i;
                i += 1;
                let e = if i < end {
                    text[i..].chars().next().unwrap()
                } else {
                    '\0'
                };
                if i >= end {
                    continue;
                }
                match e {
                    '\\' => value.push('\\'),
                    '"' => value.push('"'),
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    '0' => value.push('\0'),
                    'u' => {
                        // \u{HEX}
                        let rest = &text[i + 1..end];
                        let close = rest.find('}');
                        let ok = rest.starts_with('{') && close.is_some();
                        if !ok {
                            return Err((
                                Diagnostic::error(
                                    "B1008",
                                    "invalid unicode escape; expected `\\u{HEX}`",
                                    Span::new(file, esc_start, (i + 1).min(end)),
                                ),
                                skip_string(text, i, end),
                            ));
                        }
                        let close = close.unwrap();
                        let hex = &rest[1..close];
                        let span = Span::new(file, esc_start, i + 1 + close + 1);
                        let scalar = if !hex.is_empty()
                            && hex.len() <= 6
                            && hex.chars().all(|h| h.is_ascii_hexdigit())
                        {
                            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                        } else {
                            None
                        };
                        match scalar {
                            Some(ch) => value.push(ch),
                            None => {
                                return Err((
                                    Diagnostic::error(
                                        "B1008",
                                        format!(
                                            "`\\u{{{hex}}}` is not a valid Unicode scalar value"
                                        ),
                                        span,
                                    ),
                                    skip_string(text, i, end),
                                ))
                            }
                        }
                        i = i + 1 + close + 1;
                        continue;
                    }
                    other => {
                        return Err((
                            Diagnostic::error(
                                "B1008",
                                format!("unknown escape sequence `\\{}`", other.escape_default()),
                                Span::new(file, esc_start, i + other.len_utf8()),
                            )
                            .with_help("valid escapes: \\\\ \\\" \\n \\r \\t \\0 \\u{HEX}"),
                            skip_string(text, i, end),
                        ))
                    }
                }
                i += e.len_utf8();
            }
            _ => {
                value.push(c);
                i += c.len_utf8();
            }
        }
    }
}

/// Recovery: skip to just past the closing quote (or end of line).
fn skip_string(text: &str, mut i: usize, end: usize) -> usize {
    let bytes = text.as_bytes();
    while i < end {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use beaver_source::SourceMap;
    use std::path::PathBuf;

    fn toks(src: &str) -> Result<Vec<Tok>, Vec<Diagnostic>> {
        let mut sm = SourceMap::new();
        let id = sm.add("t.bv", PathBuf::new(), src);
        lex(id, sm.file(id)).map(|t| t.into_iter().map(|t| t.tok).collect())
    }

    fn err_code(src: &str) -> &'static str {
        toks(src).unwrap_err()[0].code
    }

    #[test]
    fn indent_and_dedent() {
        let t = toks("fn main():\n    pass\n").unwrap();
        assert_eq!(
            t,
            vec![
                Tok::Fn,
                Tok::Ident("main".into()),
                Tok::LParen,
                Tok::RParen,
                Tok::Colon,
                Tok::Newline,
                Tok::Indent,
                Tok::Pass,
                Tok::Newline,
                Tok::Dedent,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn blank_and_comment_lines_do_not_affect_indentation() {
        let t = toks("fn f():\n\n  # odd comment\n    pass\n# c\n").unwrap();
        assert_eq!(t.iter().filter(|t| **t == Tok::Indent).count(), 1);
        assert_eq!(t.iter().filter(|t| **t == Tok::Dedent).count(), 1);
    }

    #[test]
    fn nested_dedents() {
        let t = toks("if a:\n    if b:\n        pass\nx\n").unwrap();
        assert_eq!(t.iter().filter(|t| **t == Tok::Dedent).count(), 2);
    }

    #[test]
    fn indentation_must_be_multiple_of_four() {
        assert_eq!(err_code("fn f():\n   pass\n"), "B1003");
    }

    #[test]
    fn inconsistent_dedent_rejected() {
        assert_eq!(err_code("fn f():\n        pass\n    pass\n"), "B1004");
    }

    #[test]
    fn tabs_in_indentation_rejected() {
        assert_eq!(err_code("fn f():\n\tpass\n"), "B1002");
    }

    #[test]
    fn crlf_accepted() {
        assert!(toks("fn f():\r\n    pass\r\n").is_ok());
    }

    #[test]
    fn stray_cr_rejected() {
        assert_eq!(err_code("let a = 1\r let b = 2\n"), "B1016");
    }

    #[test]
    fn string_escapes() {
        let t = toks(r#"x = "a\n\t\\\"\0\u{48}\u{1F600}""#).unwrap();
        assert_eq!(t[2], Tok::Str("a\n\t\\\"\0H\u{1F600}".into()));
    }

    #[test]
    fn invalid_escapes_rejected() {
        assert_eq!(err_code(r#"x = "\q""#), "B1008");
        assert_eq!(err_code(r#"x = "\u{D800}""#), "B1008");
        assert_eq!(err_code(r#"x = "\u{110000}""#), "B1008");
        assert_eq!(err_code(r#"x = "\u41""#), "B1008");
    }

    #[test]
    fn unterminated_string() {
        assert_eq!(err_code("x = \"abc\n"), "B1007");
    }

    #[test]
    fn hash_inside_string_is_not_comment() {
        let t = toks("x = \"a # b\" # real\n").unwrap();
        assert_eq!(t[2], Tok::Str("a # b".into()));
    }

    #[test]
    fn numbers() {
        let t = toks("x = 12 + 1.5\n").unwrap();
        assert_eq!(t[2], Tok::Int(12));
        assert_eq!(t[4], Tok::Float(1.5));
        let t = toks("x = 9223372036854775807\n").unwrap();
        assert_eq!(t[2], Tok::Int(i64::MAX));
        assert_eq!(err_code("x = 9223372036854775808\n"), "B1010");
        assert_eq!(err_code("x = 12abc\n"), "B1011");
    }

    #[test]
    fn integer_followed_by_field_dot_is_two_tokens() {
        let t = toks("x = a[0].b\n").unwrap();
        assert!(t.contains(&Tok::Dot));
    }

    #[test]
    fn operators() {
        let t = toks("a += b -> c <= d != e == f\n").unwrap();
        assert_eq!(
            &t[..11],
            &[
                Tok::Ident("a".into()),
                Tok::PlusAssign,
                Tok::Ident("b".into()),
                Tok::Arrow,
                Tok::Ident("c".into()),
                Tok::Le,
                Tok::Ident("d".into()),
                Tok::NotEq,
                Tok::Ident("e".into()),
                Tok::EqEq,
                Tok::Ident("f".into()),
            ]
        );
    }

    #[test]
    fn pointer_syntax_rejected() {
        assert_eq!(err_code("x = &y\n"), "B1009");
        assert_eq!(err_code("x = {}\n"), "B1009");
    }

    #[test]
    fn non_ascii_identifiers_rejected_but_allowed_in_strings_and_comments() {
        assert_eq!(err_code("café = 1\n"), "B1009");
        assert!(toks("x = \"café\" # ünïcode\n").is_ok());
    }

    #[test]
    fn eof_without_trailing_newline() {
        let t = toks("fn f():\n    pass").unwrap();
        assert_eq!(t.last(), Some(&Tok::Eof));
        assert!(t.contains(&Tok::Dedent));
    }
}
