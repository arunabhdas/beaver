//! Semantic types.

use beaver_source::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecordId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Byte,
    String,
    Void,
    Buffer,
    Record(RecordId),
    List(Box<Ty>),
    Result(Box<Ty>),
    /// Produced after an error has been reported; compatible with everything
    /// so that one mistake does not cascade.
    Error,
}

#[derive(Debug, Clone)]
pub struct RecordDef {
    pub name: String,
    pub module: usize,
    pub public: bool,
    pub fields: Vec<(String, Ty)>,
    pub span: Span,
    /// Stable, collision-resistant symbol used by the backend.
    pub mangled: String,
    pub is_copy: bool,
    pub supports_eq: bool,
}

impl Ty {
    pub fn is_error(&self) -> bool {
        matches!(self, Ty::Error)
    }

    pub fn is_copy(&self, records: &[RecordDef]) -> bool {
        match self {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Byte | Ty::Error => true,
            Ty::Record(r) => records[r.0].is_copy,
            Ty::String | Ty::Void | Ty::Buffer | Ty::List(_) | Ty::Result(_) => false,
        }
    }

    /// Does dropping a value of this type have any effect?
    pub fn needs_drop(&self, records: &[RecordDef]) -> bool {
        !self.is_copy(records) && *self != Ty::Void
    }

    pub fn supports_eq(&self, records: &[RecordDef]) -> bool {
        match self {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Byte | Ty::String | Ty::Error => true,
            Ty::Record(r) => records[r.0].supports_eq,
            _ => false,
        }
    }

    pub fn is_printable(&self) -> bool {
        matches!(
            self,
            Ty::Int | Ty::Float | Ty::Bool | Ty::Byte | Ty::String | Ty::Error
        )
    }

    /// Exact type equality, treating `Error` as a wildcard.
    pub fn compatible(&self, other: &Ty) -> bool {
        match (self, other) {
            (Ty::Error, _) | (_, Ty::Error) => true,
            (Ty::List(a), Ty::List(b)) | (Ty::Result(a), Ty::Result(b)) => a.compatible(b),
            (a, b) => a == b,
        }
    }

    pub fn display(&self, records: &[RecordDef]) -> String {
        match self {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Bool => "Bool".into(),
            Ty::Byte => "Byte".into(),
            Ty::String => "String".into(),
            Ty::Void => "Void".into(),
            Ty::Buffer => "Buffer".into(),
            Ty::Record(r) => records[r.0].name.clone(),
            Ty::List(t) => format!("List[{}]", t.display(records)),
            Ty::Result(t) => format!("Result[{}]", t.display(records)),
            Ty::Error => "{unknown}".into(),
        }
    }
}
