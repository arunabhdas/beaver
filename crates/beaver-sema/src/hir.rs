//! Typed, name-resolved high-level IR produced by the type checker and
//! validated by the ownership checker.

use crate::types::{RecordDef, RecordId, Ty};
use beaver_source::{SourceMap, Span};
pub use beaver_syntax::ast::{AssignOp, BinOp, BorrowKind, UnOp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalKind {
    /// Owns its value. `mutable` is `var`.
    Owned { mutable: bool },
    /// Non-owning reference produced by `view` (`mutable: false`) or `edit`.
    Ref { mutable: bool },
}

#[derive(Debug, Clone)]
pub struct LocalInfo {
    pub name: String,
    /// Type of the value (for references: the referent's type).
    pub ty: Ty,
    pub kind: LocalKind,
    pub span: Span,
    pub is_param: bool,
}

impl LocalInfo {
    pub fn is_ref(&self) -> bool {
        matches!(self.kind, LocalKind::Ref { .. })
    }
}

/// How an argument is passed to a user function parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassMode {
    /// Copy type passed by value.
    Copy,
    /// `take` parameter: ownership moves into the callee.
    Move,
    /// Read-only (`mutable: false`) or exclusive (`mutable: true`) borrow.
    Borrow { mutable: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Print,
    Assert,
    AssertEq,
    Len,
    Append,
    Clone,
    IsOk,
    Unwrap,
    UnwrapError,
    IntToFloat,
    FloatToInt,
    IntToByte,
    ByteToInt,
}

impl Builtin {
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Print => "print",
            Builtin::Assert => "assert",
            Builtin::AssertEq => "assert_eq",
            Builtin::Len => "len",
            Builtin::Append => "append",
            Builtin::Clone => "clone",
            Builtin::IsOk => "is_ok",
            Builtin::Unwrap => "unwrap",
            Builtin::UnwrapError => "unwrap_error",
            Builtin::IntToFloat => "Float",
            Builtin::FloatToInt | Builtin::ByteToInt => "Int",
            Builtin::IntToByte => "Byte",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Local(LocalId),
    Field(Box<Expr>, usize),
    Index(Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `view place` / `edit place`; only in `let` initializers and arguments.
    Borrow(BorrowKind, Box<Expr>),
    /// `take place`: moves the place into a fresh temporary.
    Take(Box<Expr>),
    Try(Box<Expr>),
    Call(FnId, Vec<(PassMode, Expr)>),
    Builtin(Builtin, Vec<Expr>),
    /// Field initializers in source (evaluation) order.
    Construct(RecordId, Vec<(usize, Expr)>),
    List(Vec<Expr>),
    BufferNew(Box<Expr>),
    ResultOk(Option<Box<Expr>>),
    ResultFail(Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Ty,
    pub span: Span,
}

impl Expr {
    /// True for `local`, `place.field`, `place[index]` chains rooted at a local.
    pub fn is_place(&self) -> bool {
        match &self.kind {
            ExprKind::Local(_) => true,
            ExprKind::Field(b, _) | ExprKind::Index(b, _) => b.is_place(),
            _ => false,
        }
    }

    pub fn place_root(&self) -> Option<LocalId> {
        match &self.kind {
            ExprKind::Local(l) => Some(*l),
            ExprKind::Field(b, _) | ExprKind::Index(b, _) => b.place_root(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Let {
        local: LocalId,
        init: Expr,
    },
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
    },
    Return(Option<Expr>),
    If {
        cond: Expr,
        then: Block,
        els: Option<Block>,
    },
    While {
        cond: Expr,
        body: Block,
    },
    ForRange {
        var: LocalId,
        start: Expr,
        end: Expr,
        body: Block,
    },
    ForList {
        var: LocalId,
        iter: Expr,
        body: Block,
    },
    Break,
    Continue,
    Scope(Block),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub local: LocalId,
    pub pass: PassMode,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub module: usize,
    pub public: bool,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub locals: Vec<LocalInfo>,
    pub body: Block,
    pub span: Span,
    pub mangled: String,
    /// `Some(label)` for `test "label":` blocks.
    pub test_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub file: String,
    pub func: FnId,
}

/// A fully type-checked and ownership-checked program.
pub struct Program {
    pub sources: SourceMap,
    pub records: Vec<RecordDef>,
    pub functions: Vec<Function>,
    pub entry: Option<FnId>,
    pub tests: Vec<TestCase>,
    pub package_name: String,
}
