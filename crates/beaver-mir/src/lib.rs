//! Beaver typed MIR.
//!
//! The MIR is a control-flow graph in which evaluation order, ownership
//! transfers (`Operand::Move`), borrows (`Rvalue::Ref`), borrow ends
//! (`Stmt::EndBorrow`) and destruction (`Stmt::Drop`) are all explicit, so a
//! backend never has to infer them. A [`MirProgram`] can only be produced from
//! a [`beaver_sema::hir::Program`], which itself only exists after type and
//! ownership checking succeeded.

pub mod lower;
pub mod mir;

pub use lower::lower;
pub use mir::*;
