use beaver_source::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    // keywords
    Fn,
    Type,
    Pub,
    Const,
    Let,
    Var,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Scope,
    Pass,
    Return,
    Use,
    Test,
    True,
    False,
    And,
    Or,
    Not,
    View,
    Edit,
    Take,
    Try,
    // punctuation
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Arrow,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    // layout
    Newline,
    Indent,
    Dedent,
    Eof,
}

impl Tok {
    pub fn keyword(word: &str) -> Option<Tok> {
        Some(match word {
            "fn" => Tok::Fn,
            "type" => Tok::Type,
            "pub" => Tok::Pub,
            "const" => Tok::Const,
            "let" => Tok::Let,
            "var" => Tok::Var,
            "if" => Tok::If,
            "elif" => Tok::Elif,
            "else" => Tok::Else,
            "while" => Tok::While,
            "for" => Tok::For,
            "in" => Tok::In,
            "break" => Tok::Break,
            "continue" => Tok::Continue,
            "scope" => Tok::Scope,
            "pass" => Tok::Pass,
            "return" => Tok::Return,
            "use" => Tok::Use,
            "test" => Tok::Test,
            "true" => Tok::True,
            "false" => Tok::False,
            "and" => Tok::And,
            "or" => Tok::Or,
            "not" => Tok::Not,
            "view" => Tok::View,
            "edit" => Tok::Edit,
            "take" => Tok::Take,
            "try" => Tok::Try,
            _ => return None,
        })
    }

    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            Tok::Fn
                | Tok::Type
                | Tok::Pub
                | Tok::Const
                | Tok::Let
                | Tok::Var
                | Tok::If
                | Tok::Elif
                | Tok::Else
                | Tok::While
                | Tok::For
                | Tok::In
                | Tok::Break
                | Tok::Continue
                | Tok::Scope
                | Tok::Pass
                | Tok::Return
                | Tok::Use
                | Tok::Test
                | Tok::True
                | Tok::False
                | Tok::And
                | Tok::Or
                | Tok::Not
                | Tok::View
                | Tok::Edit
                | Tok::Take
                | Tok::Try
        )
    }

    /// Human-readable description used in parse errors.
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("identifier `{s}`"),
            Tok::Int(v) => format!("integer `{v}`"),
            Tok::Float(v) => format!("float `{v}`"),
            Tok::Str(_) => "string literal".to_string(),
            Tok::Newline => "end of line".to_string(),
            Tok::Indent => "indentation".to_string(),
            Tok::Dedent => "end of block".to_string(),
            Tok::Eof => "end of file".to_string(),
            other => format!("`{}`", other.text()),
        }
    }

    /// Canonical spelling of fixed tokens.
    pub fn text(&self) -> &'static str {
        match self {
            Tok::Fn => "fn",
            Tok::Type => "type",
            Tok::Pub => "pub",
            Tok::Const => "const",
            Tok::Let => "let",
            Tok::Var => "var",
            Tok::If => "if",
            Tok::Elif => "elif",
            Tok::Else => "else",
            Tok::While => "while",
            Tok::For => "for",
            Tok::In => "in",
            Tok::Break => "break",
            Tok::Continue => "continue",
            Tok::Scope => "scope",
            Tok::Pass => "pass",
            Tok::Return => "return",
            Tok::Use => "use",
            Tok::Test => "test",
            Tok::True => "true",
            Tok::False => "false",
            Tok::And => "and",
            Tok::Or => "or",
            Tok::Not => "not",
            Tok::View => "view",
            Tok::Edit => "edit",
            Tok::Take => "take",
            Tok::Try => "try",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::Comma => ",",
            Tok::Colon => ":",
            Tok::Dot => ".",
            Tok::Arrow => "->",
            Tok::Assign => "=",
            Tok::PlusAssign => "+=",
            Tok::MinusAssign => "-=",
            Tok::StarAssign => "*=",
            Tok::SlashAssign => "/=",
            Tok::EqEq => "==",
            Tok::NotEq => "!=",
            Tok::Lt => "<",
            Tok::Le => "<=",
            Tok::Gt => ">",
            Tok::Ge => ">=",
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::Ident(_) | Tok::Int(_) | Tok::Float(_) | Tok::Str(_) => "<literal>",
            Tok::Newline => "<newline>",
            Tok::Indent => "<indent>",
            Tok::Dedent => "<dedent>",
            Tok::Eof => "<eof>",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}
