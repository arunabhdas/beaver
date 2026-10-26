//! MIR data structures.

use beaver_sema::hir::BinOp;
use beaver_sema::types::{RecordDef, RecordId, Ty};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Local(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bb(pub usize);

/// Source location attached to runtime checks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Loc {
    pub file: String,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub struct MirLocal {
    pub ty: Ty,
    /// References are stored as C pointers and auto-dereferenced in places.
    pub is_ref: bool,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Proj {
    Field(usize),
    /// Bounds-checked element access; the index is a local holding an Int.
    Index(Local, Loc),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub local: Local,
    pub proj: Vec<Proj>,
}

impl Place {
    pub fn local(l: Local) -> Place {
        Place {
            local: l,
            proj: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// Bitwise copy of a Copy value (or of a reference pointer).
    Copy(Place),
    /// Ownership transfer; the source is never dropped afterwards.
    Move(Place),
    /// The pointer held by a reference local (passing a borrow to a callee).
    Ref(Local),
    Const(Const),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone)]
pub enum Rvalue {
    Use(Operand),
    CheckedInt(IntOp, Operand, Operand, Loc),
    CheckedNeg(Operand, Loc),
    Float(FloatOp, Operand, Operand),
    FloatNeg(Operand),
    Not(Operand),
    /// Scalar comparison of Int/Float/Byte/Bool operands.
    Compare(BinOp, Operand, Operand),
    /// Structural equality (String / record) of two places.
    Equal {
        ty: Ty,
        a: Place,
        b: Place,
        negate: bool,
    },
    Concat(Place, Place),
    /// Field operands in declaration order.
    Construct(RecordId, Vec<Operand>),
    ListLit(Ty, Vec<Operand>),
    BufferNew(Operand, Loc),
    /// `ty` is the full `Result[T]` type.
    ResultOk(Ty, Option<Operand>),
    ResultFail(Ty, Operand),
    Call(usize, Vec<Operand>),
    Clone(Place),
    Len(Place),
    IsOk(Place),
    /// Consume a Result, yielding its success payload or trapping.
    Unwrap(Operand, Loc),
    UnwrapError(Operand, Loc),
    /// Move the payload out of a Result already known to be ok / failed.
    ResultTakeOk(Place),
    ResultTakeErr(Place),
    IntToFloat(Operand),
    FloatToInt(Operand, Loc),
    IntToByte(Operand, Loc),
    ByteToInt(Operand),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Assign(Place, Rvalue),
    /// Point reference local at a place (`let r = view x`, call borrows).
    BindRef(Local, Place),
    /// Evaluate for side effects only (Void calls, Result[Void] unwraps).
    Eval(Rvalue),
    Drop(Place),
    /// Marks the end of a named borrow's lexical lifetime (no code).
    EndBorrow(Local),
    Append(Place, Operand),
    Print(Place),
    Assert(Operand, Loc),
    AssertEq(Place, Place, Loc),
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Goto(Bb),
    Branch(Operand, Bb, Bb),
    Return,
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub stmts: Vec<Stmt>,
    pub term: Terminator,
}

#[derive(Debug, Clone)]
pub struct MirFn {
    pub name: String,
    pub mangled: String,
    pub params: Vec<Local>,
    pub ret: Ty,
    /// Holds the return value for non-Void functions.
    pub ret_local: Option<Local>,
    pub locals: Vec<MirLocal>,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Debug, Clone)]
pub struct MirTest {
    pub name: String,
    pub file: String,
    pub func: usize,
}

/// MIR for a whole program. Only constructible from a fully checked
/// [`beaver_sema::hir::Program`] via [`crate::lower`].
pub struct MirProgram {
    pub package_name: String,
    pub records: Vec<RecordDef>,
    pub functions: Vec<MirFn>,
    pub entry: Option<usize>,
    pub tests: Vec<MirTest>,
}

impl MirFn {
    pub fn place_ty(&self, p: &Place, records: &[RecordDef]) -> Ty {
        let mut ty = self.locals[p.local.0].ty.clone();
        for pr in &p.proj {
            ty = match (pr, &ty) {
                (Proj::Field(i), Ty::Record(r)) => records[r.0].fields[*i].1.clone(),
                (Proj::Index(..), Ty::List(t)) => (**t).clone(),
                (Proj::Index(..), Ty::Buffer) => Ty::Byte,
                _ => Ty::Error,
            };
        }
        ty
    }

    pub fn operand_ty(&self, o: &Operand, records: &[RecordDef]) -> Ty {
        match o {
            Operand::Copy(p) | Operand::Move(p) => self.place_ty(p, records),
            Operand::Ref(l) => self.locals[l.0].ty.clone(),
            Operand::Const(Const::Int(_)) => Ty::Int,
            Operand::Const(Const::Float(_)) => Ty::Float,
            Operand::Const(Const::Bool(_)) => Ty::Bool,
            Operand::Const(Const::Str(_)) => Ty::String,
        }
    }
}

// ----- textual dump (debugging and tests) -------------------------------

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_{}", self.local.0)?;
        for p in &self.proj {
            match p {
                Proj::Field(i) => write!(f, ".{i}")?,
                Proj::Index(l, _) => write!(f, "[_{}]", l.0)?,
            }
        }
        Ok(())
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Copy(p) => write!(f, "copy {p}"),
            Operand::Move(p) => write!(f, "move {p}"),
            Operand::Ref(l) => write!(f, "ref _{}", l.0),
            Operand::Const(Const::Int(v)) => write!(f, "{v}"),
            Operand::Const(Const::Float(v)) => write!(f, "{v:?}"),
            Operand::Const(Const::Bool(v)) => write!(f, "{v}"),
            Operand::Const(Const::Str(s)) => write!(f, "{s:?}"),
        }
    }
}

fn ops(v: &[Operand]) -> String {
    v.iter()
        .map(|o| o.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for Rvalue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rvalue::Use(o) => write!(f, "{o}"),
            Rvalue::CheckedInt(op, a, b, _) => write!(f, "checked_{op:?}({a}, {b})"),
            Rvalue::CheckedNeg(a, _) => write!(f, "checked_neg({a})"),
            Rvalue::Float(op, a, b) => write!(f, "f{op:?}({a}, {b})"),
            Rvalue::FloatNeg(a) => write!(f, "fneg({a})"),
            Rvalue::Not(a) => write!(f, "not({a})"),
            Rvalue::Compare(op, a, b) => write!(f, "{a} {} {b}", op.symbol()),
            Rvalue::Equal { a, b, negate, .. } => {
                write!(f, "{}eq({a}, {b})", if *negate { "!" } else { "" })
            }
            Rvalue::Concat(a, b) => write!(f, "concat({a}, {b})"),
            Rvalue::Construct(r, v) => write!(f, "record#{}({})", r.0, ops(v)),
            Rvalue::ListLit(_, v) => write!(f, "list[{}]", ops(v)),
            Rvalue::BufferNew(o, _) => write!(f, "buffer({o})"),
            Rvalue::ResultOk(_, o) => match o {
                Some(o) => write!(f, "ok({o})"),
                None => write!(f, "ok()"),
            },
            Rvalue::ResultFail(_, o) => write!(f, "fail({o})"),
            Rvalue::Call(id, v) => write!(f, "call fn#{id}({})", ops(v)),
            Rvalue::Clone(p) => write!(f, "clone({p})"),
            Rvalue::Len(p) => write!(f, "len({p})"),
            Rvalue::IsOk(p) => write!(f, "is_ok({p})"),
            Rvalue::Unwrap(o, _) => write!(f, "unwrap({o})"),
            Rvalue::UnwrapError(o, _) => write!(f, "unwrap_error({o})"),
            Rvalue::ResultTakeOk(p) => write!(f, "take_ok({p})"),
            Rvalue::ResultTakeErr(p) => write!(f, "take_err({p})"),
            Rvalue::IntToFloat(o) => write!(f, "Float({o})"),
            Rvalue::FloatToInt(o, _) => write!(f, "Int({o})"),
            Rvalue::IntToByte(o, _) => write!(f, "Byte({o})"),
            Rvalue::ByteToInt(o) => write!(f, "Int({o})"),
        }
    }
}

impl fmt::Display for MirFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fn {}:", self.name)?;
        for (i, b) in self.blocks.iter().enumerate() {
            writeln!(f, "  bb{i}:")?;
            for s in &b.stmts {
                match s {
                    Stmt::Assign(p, r) => writeln!(f, "    {p} = {r}")?,
                    Stmt::BindRef(l, p) => writeln!(f, "    _{} = &{p}", l.0)?,
                    Stmt::Eval(r) => writeln!(f, "    eval {r}")?,
                    Stmt::Drop(p) => writeln!(f, "    drop {p}")?,
                    Stmt::EndBorrow(l) => writeln!(f, "    end_borrow _{}", l.0)?,
                    Stmt::Append(p, o) => writeln!(f, "    append({p}, {o})")?,
                    Stmt::Print(p) => writeln!(f, "    print({p})")?,
                    Stmt::Assert(o, _) => writeln!(f, "    assert({o})")?,
                    Stmt::AssertEq(a, b, _) => writeln!(f, "    assert_eq({a}, {b})")?,
                }
            }
            match &b.term {
                Terminator::Goto(t) => writeln!(f, "    goto bb{}", t.0)?,
                Terminator::Branch(c, t, e) => {
                    writeln!(f, "    if {c} goto bb{} else bb{}", t.0, e.0)?
                }
                Terminator::Return => writeln!(f, "    return")?,
                Terminator::Unreachable => writeln!(f, "    unreachable")?,
            }
        }
        Ok(())
    }
}
