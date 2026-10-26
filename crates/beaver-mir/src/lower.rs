//! HIR -> MIR lowering with drop elaboration.
//!
//! Ownership bookkeeping mirrors the ownership checker: `moved` holds the MIR
//! locals (named locals and temporaries) whose value has been transferred on
//! the current path. Owned locals are dropped when their scope ends on every
//! exit (fallthrough, `return`, `try` error return, `break`, `continue`).
//! Statement temporaries are dropped at the end of their statement unless
//! they were consumed. At control-flow joins, values that are moved on some
//! incoming paths only are dropped on the other paths before the join (the
//! ownership checker has proven they are not used afterwards).

use crate::mir::*;
use beaver_sema::hir::{self, BinOp, ExprKind as EK, PassMode, StmtKind as SK, UnOp};
use beaver_sema::types::Ty;
use beaver_source::Span;
use std::collections::BTreeSet;

pub fn lower(prog: &hir::Program) -> MirProgram {
    let functions = prog
        .functions
        .iter()
        .map(|f| FnBuilder::new(prog, f).build())
        .collect();
    MirProgram {
        package_name: prog.package_name.clone(),
        records: prog.records.clone(),
        functions,
        entry: prog.entry.map(|f| f.0),
        tests: prog
            .tests
            .iter()
            .map(|t| MirTest {
                name: t.name.clone(),
                file: t.file.clone(),
                func: t.func.0,
            })
            .collect(),
    }
}

#[derive(Default)]
struct Scope {
    owned: Vec<Local>,
    refs: Vec<Local>,
}

struct LoopFrame {
    cont: Bb,
    body_depth: usize,
    breaks: Vec<(usize, BTreeSet<Local>)>,
}

struct FnBuilder<'a> {
    prog: &'a hir::Program,
    f: &'a hir::Function,
    locals: Vec<MirLocal>,
    blocks: Vec<BasicBlock>,
    cur: usize,
    diverged: bool,
    scopes: Vec<Scope>,
    temps: Vec<Vec<Local>>,
    moved: BTreeSet<Local>,
    loops: Vec<LoopFrame>,
    ret_local: Option<Local>,
}

impl<'a> FnBuilder<'a> {
    fn new(prog: &'a hir::Program, f: &'a hir::Function) -> Self {
        let locals = f
            .locals
            .iter()
            .map(|l| MirLocal {
                ty: l.ty.clone(),
                is_ref: l.is_ref(),
                name: Some(l.name.clone()),
            })
            .collect();
        FnBuilder {
            prog,
            f,
            locals,
            blocks: vec![BasicBlock {
                stmts: Vec::new(),
                term: Terminator::Unreachable,
            }],
            cur: 0,
            diverged: false,
            scopes: Vec::new(),
            temps: Vec::new(),
            moved: BTreeSet::new(),
            loops: Vec::new(),
            ret_local: None,
        }
    }

    fn build(mut self) -> MirFn {
        if self.f.ret != Ty::Void {
            self.ret_local = Some(self.new_local(self.f.ret.clone(), false, None));
        }
        let mut fscope = Scope::default();
        for p in &self.f.params {
            let l = Local(p.local.0);
            if p.pass == PassMode::Move && self.needs_drop(&self.f.locals[p.local.0].ty) {
                fscope.owned.push(l);
            }
        }
        self.scopes.push(fscope);
        for s in &self.f.body.stmts {
            self.stmt(s);
        }
        if !self.diverged {
            self.drop_scopes_from(0);
            if self.f.ret == Ty::Void {
                self.terminate(Terminator::Return);
            }
        }
        self.scopes.pop();
        MirFn {
            name: self.f.name.clone(),
            mangled: self.f.mangled.clone(),
            params: self.f.params.iter().map(|p| Local(p.local.0)).collect(),
            ret: self.f.ret.clone(),
            ret_local: self.ret_local,
            locals: self.locals,
            blocks: self.blocks,
        }
    }

    // ----- infrastructure ---------------------------------------------

    fn needs_drop(&self, ty: &Ty) -> bool {
        ty.needs_drop(&self.prog.records)
    }

    fn is_copy(&self, ty: &Ty) -> bool {
        ty.is_copy(&self.prog.records)
    }

    fn loc(&self, span: Span) -> Loc {
        let (file, line, col) = self.prog.sources.location(span);
        Loc {
            file,
            line: line as u32,
            col: col as u32,
        }
    }

    fn new_local(&mut self, ty: Ty, is_ref: bool, name: Option<String>) -> Local {
        self.locals.push(MirLocal { ty, is_ref, name });
        Local(self.locals.len() - 1)
    }

    fn temp(&mut self, ty: Ty) -> Local {
        self.new_local(ty, false, None)
    }

    /// A temporary that must be dropped at the end of the statement unless
    /// it is consumed first.
    fn live_temp(&mut self, ty: Ty) -> Local {
        let nd = self.needs_drop(&ty);
        let t = self.temp(ty);
        if nd {
            self.temps
                .last_mut()
                .expect("temporary outside a statement")
                .push(t);
        }
        t
    }

    fn new_block(&mut self) -> Bb {
        self.blocks.push(BasicBlock {
            stmts: Vec::new(),
            term: Terminator::Unreachable,
        });
        Bb(self.blocks.len() - 1)
    }

    fn emit(&mut self, s: Stmt) {
        self.blocks[self.cur].stmts.push(s);
    }

    fn assign(&mut self, p: Place, r: Rvalue) {
        self.emit(Stmt::Assign(p, r));
    }

    fn terminate(&mut self, t: Terminator) {
        self.blocks[self.cur].term = t;
    }

    fn switch(&mut self, b: Bb) {
        self.cur = b.0;
    }

    /// After a diverging terminator, continue emitting into a dead block.
    fn start_dead(&mut self) {
        let b = self.new_block();
        self.switch(b);
        self.diverged = true;
    }

    fn mark_moved(&mut self, op: &Operand) {
        if let Operand::Move(p) = op {
            if p.proj.is_empty() {
                self.moved.insert(p.local);
            }
        }
    }

    fn drop_if_owned(&mut self, l: Local) {
        if !self.moved.contains(&l) {
            self.emit(Stmt::Drop(Place::local(l)));
        }
    }

    fn drop_scopes_from(&mut self, depth: usize) {
        let mut order = Vec::new();
        for sc in self.scopes[depth..].iter().rev() {
            for l in sc.owned.iter().rev() {
                order.push(*l);
            }
        }
        for l in order {
            self.drop_if_owned(l);
        }
    }

    /// Drops for leaving the whole function from the middle of a statement.
    fn drop_everything(&mut self) {
        let mut temps = Vec::new();
        for ts in self.temps.iter().rev() {
            for t in ts.iter().rev() {
                temps.push(*t);
            }
        }
        for t in temps {
            self.drop_if_owned(t);
        }
        self.drop_scopes_from(0);
    }

    fn push_temps(&mut self) {
        self.temps.push(Vec::new());
    }

    fn pop_temps(&mut self) {
        let ts = self.temps.pop().unwrap();
        if !self.diverged {
            for t in ts.iter().rev() {
                self.drop_if_owned(*t);
            }
        }
        for t in ts {
            self.moved.remove(&t);
        }
    }

    fn enter_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn exit_scope(&mut self) {
        let sc = self.scopes.pop().unwrap();
        if !self.diverged {
            for l in sc.owned.iter().rev() {
                self.drop_if_owned(*l);
            }
            for r in sc.refs.iter().rev() {
                self.emit(Stmt::EndBorrow(*r));
            }
        }
        for l in sc.owned {
            self.moved.remove(&l);
        }
    }

    fn join(&mut self, ends: Vec<(usize, BTreeSet<Local>, bool)>, target: Bb) {
        let live: Vec<(usize, BTreeSet<Local>)> = ends
            .into_iter()
            .filter(|e| !e.2)
            .map(|e| (e.0, e.1))
            .collect();
        if live.is_empty() {
            self.switch(target);
            self.diverged = true;
            return;
        }
        let mut union = BTreeSet::new();
        for (_, s) in &live {
            union.extend(s.iter().copied());
        }
        for (bb, state) in &live {
            self.cur = *bb;
            let fix: Vec<Local> = union.difference(state).copied().collect();
            for l in fix.into_iter().rev() {
                if self.needs_drop(&self.locals[l.0].ty.clone()) {
                    self.emit(Stmt::Drop(Place::local(l)));
                }
            }
            self.terminate(Terminator::Goto(target));
        }
        self.moved = union;
        self.switch(target);
        self.diverged = false;
    }

    // ----- statements -------------------------------------------------

    fn block(&mut self, b: &hir::Block) {
        self.enter_scope();
        for s in &b.stmts {
            self.stmt(s);
        }
        self.exit_scope();
    }

    fn stmt(&mut self, s: &hir::Stmt) {
        match &s.kind {
            SK::Let { local, init } => {
                let ml = Local(local.0);
                self.push_temps();
                if let EK::Borrow(_, place) = &init.kind {
                    let p = self.place(place);
                    self.emit(Stmt::BindRef(ml, p));
                    self.scopes.last_mut().unwrap().refs.push(ml);
                } else {
                    let op = self.value(init);
                    self.mark_moved(&op);
                    self.assign(Place::local(ml), Rvalue::Use(op));
                }
                self.pop_temps();
                if !self.locals[ml.0].is_ref && self.needs_drop(&self.locals[ml.0].ty.clone()) {
                    self.scopes.last_mut().unwrap().owned.push(ml);
                    self.moved.remove(&ml);
                }
            }
            SK::Assign { target, op, value } => {
                self.push_temps();
                if *op == hir::AssignOp::Set {
                    let v = self.value(value);
                    let p = self.place(target);
                    if self.needs_drop(&target.ty) {
                        self.emit(Stmt::Drop(p.clone()));
                    }
                    self.mark_moved(&v);
                    self.assign(p, Rvalue::Use(v));
                } else {
                    let p = self.place(target);
                    let cur = self.temp(target.ty.clone());
                    self.assign(Place::local(cur), Rvalue::Use(Operand::Copy(p.clone())));
                    let rhs = self.value(value);
                    let loc = self.loc(s.span);
                    let a = Operand::Copy(Place::local(cur));
                    let r = if target.ty == Ty::Float {
                        let fop = match op {
                            hir::AssignOp::Add => FloatOp::Add,
                            hir::AssignOp::Sub => FloatOp::Sub,
                            hir::AssignOp::Mul => FloatOp::Mul,
                            _ => FloatOp::Div,
                        };
                        Rvalue::Float(fop, a, rhs)
                    } else {
                        let iop = match op {
                            hir::AssignOp::Add => IntOp::Add,
                            hir::AssignOp::Sub => IntOp::Sub,
                            hir::AssignOp::Mul => IntOp::Mul,
                            _ => IntOp::Div,
                        };
                        Rvalue::CheckedInt(iop, a, rhs, loc)
                    };
                    let res = self.temp(target.ty.clone());
                    self.assign(Place::local(res), r);
                    self.assign(p, Rvalue::Use(Operand::Copy(Place::local(res))));
                }
                self.pop_temps();
            }
            SK::Return(e) => {
                self.push_temps();
                if let Some(e) = e {
                    let op = self.value(e);
                    self.mark_moved(&op);
                    let rl = self.ret_local.expect("return value in Void function");
                    self.assign(Place::local(rl), Rvalue::Use(op));
                }
                self.drop_everything();
                self.terminate(Terminator::Return);
                self.temps.pop();
                self.start_dead();
            }
            SK::If { cond, then, els } => {
                self.push_temps();
                let c = self.value(cond);
                self.pop_temps();
                let (tb, eb, jb) = (self.new_block(), self.new_block(), self.new_block());
                self.terminate(Terminator::Branch(c, tb, eb));
                let before = self.moved.clone();
                self.switch(tb);
                self.diverged = false;
                self.block(then);
                let a = (self.cur, self.moved.clone(), self.diverged);
                self.moved = before;
                self.switch(eb);
                self.diverged = false;
                if let Some(e) = els {
                    self.block(e);
                }
                let b = (self.cur, self.moved.clone(), self.diverged);
                self.join(vec![a, b], jb);
            }
            SK::While { cond, body } => {
                let head = self.new_block();
                self.terminate(Terminator::Goto(head));
                self.switch(head);
                self.push_temps();
                let c = self.value(cond);
                self.pop_temps();
                let (body_bb, exit_bb, after) =
                    (self.new_block(), self.new_block(), self.new_block());
                self.terminate(Terminator::Branch(c, body_bb, exit_bb));
                let entry = self.moved.clone();
                self.loops.push(LoopFrame {
                    cont: head,
                    body_depth: self.scopes.len(),
                    breaks: Vec::new(),
                });
                self.switch(body_bb);
                self.diverged = false;
                self.block(body);
                if !self.diverged {
                    self.terminate(Terminator::Goto(head));
                }
                let frame = self.loops.pop().unwrap();
                let mut ends: Vec<(usize, BTreeSet<Local>, bool)> = frame
                    .breaks
                    .into_iter()
                    .map(|(b, s)| (b, s, false))
                    .collect();
                if !matches!(cond.kind, EK::Bool(true)) {
                    ends.push((exit_bb.0, entry.clone(), false));
                }
                self.moved = entry;
                self.join(ends, after);
            }
            SK::ForRange {
                var,
                start,
                end,
                body,
            } => {
                self.push_temps();
                let s0 = self.value(start);
                let e0 = self.value(end);
                let i = self.temp(Ty::Int);
                let en = self.temp(Ty::Int);
                self.assign(Place::local(i), Rvalue::Use(s0));
                self.assign(Place::local(en), Rvalue::Use(e0));
                self.pop_temps();
                let loc = self.loc(s.span);
                self.loop_common(
                    body,
                    *var,
                    s.span,
                    |b, body_bb, exit_bb| {
                        let c = b.temp(Ty::Bool);
                        b.assign(
                            Place::local(c),
                            Rvalue::Compare(
                                BinOp::Lt,
                                Operand::Copy(Place::local(i)),
                                Operand::Copy(Place::local(en)),
                            ),
                        );
                        b.terminate(Terminator::Branch(
                            Operand::Copy(Place::local(c)),
                            body_bb,
                            exit_bb,
                        ));
                    },
                    move |b, var_l| {
                        b.assign(
                            Place::local(var_l),
                            Rvalue::Use(Operand::Copy(Place::local(i))),
                        );
                    },
                    move |b| {
                        b.assign(
                            Place::local(i),
                            Rvalue::CheckedInt(
                                IntOp::Add,
                                Operand::Copy(Place::local(i)),
                                Operand::Const(Const::Int(1)),
                                loc.clone(),
                            ),
                        );
                    },
                );
            }
            SK::ForList { var, iter, body } => {
                self.enter_scope();
                self.push_temps();
                let list = if iter.is_place() {
                    let p = self.place(iter);
                    let r = self.new_local(iter.ty.clone(), true, None);
                    self.emit(Stmt::BindRef(r, p));
                    self.scopes.last_mut().unwrap().refs.push(r);
                    Place::local(r)
                } else {
                    let op = self.value(iter);
                    let Operand::Move(p) = op else {
                        unreachable!("list temporaries are owned")
                    };
                    // The temporary lives for the whole loop: move it from
                    // the statement's temporaries to the loop header scope.
                    self.temps.last_mut().unwrap().retain(|t| *t != p.local);
                    self.scopes.last_mut().unwrap().owned.push(p.local);
                    p
                };
                self.pop_temps();
                let idx = self.temp(Ty::Int);
                self.assign(
                    Place::local(idx),
                    Rvalue::Use(Operand::Const(Const::Int(0))),
                );
                let loc = self.loc(iter.span);
                let list2 = list.clone();
                let loc2 = loc.clone();
                let var_is_ref = self.locals[var.0].is_ref;
                self.loop_common(
                    body,
                    *var,
                    s.span,
                    |b, body_bb, exit_bb| {
                        let n = b.temp(Ty::Int);
                        b.assign(Place::local(n), Rvalue::Len(list.clone()));
                        let c = b.temp(Ty::Bool);
                        b.assign(
                            Place::local(c),
                            Rvalue::Compare(
                                BinOp::Lt,
                                Operand::Copy(Place::local(idx)),
                                Operand::Copy(Place::local(n)),
                            ),
                        );
                        b.terminate(Terminator::Branch(
                            Operand::Copy(Place::local(c)),
                            body_bb,
                            exit_bb,
                        ));
                    },
                    move |b, var_l| {
                        let mut elem = list2.clone();
                        elem.proj.push(Proj::Index(idx, loc2.clone()));
                        if var_is_ref {
                            b.emit(Stmt::BindRef(var_l, elem));
                        } else {
                            b.assign(Place::local(var_l), Rvalue::Use(Operand::Copy(elem)));
                        }
                    },
                    move |b| {
                        b.assign(
                            Place::local(idx),
                            Rvalue::CheckedInt(
                                IntOp::Add,
                                Operand::Copy(Place::local(idx)),
                                Operand::Const(Const::Int(1)),
                                loc.clone(),
                            ),
                        );
                    },
                );
                self.exit_scope();
            }
            SK::Break => {
                let depth = self.loops.last().unwrap().body_depth;
                self.drop_scopes_from(depth);
                let inner: BTreeSet<Local> = self.scopes[depth..]
                    .iter()
                    .flat_map(|s| s.owned.iter().copied())
                    .collect();
                let state: BTreeSet<Local> = self.moved.difference(&inner).copied().collect();
                let cur = self.cur;
                self.loops.last_mut().unwrap().breaks.push((cur, state));
                self.start_dead();
            }
            SK::Continue => {
                let (depth, cont) = {
                    let f = self.loops.last().unwrap();
                    (f.body_depth, f.cont)
                };
                self.drop_scopes_from(depth);
                self.terminate(Terminator::Goto(cont));
                self.start_dead();
            }
            SK::Scope(b) => self.block(b),
            SK::Expr(e) => {
                self.push_temps();
                if e.ty == Ty::Void {
                    self.effect(e);
                } else if e.is_place() {
                    // Evaluate the access (bounds checks still apply).
                    let p = self.place(e);
                    let t = self.new_local(e.ty.clone(), true, None);
                    self.emit(Stmt::BindRef(t, p));
                } else {
                    let _ = self.value(e);
                }
                self.pop_temps();
            }
        }
    }

    /// Shared loop skeleton: `head` (condition), body with fresh loop
    /// variable, `incr` (continue target), exit join.
    fn loop_common(
        &mut self,
        body: &hir::Block,
        var: hir::LocalId,
        _span: Span,
        head_fn: impl FnOnce(&mut Self, Bb, Bb),
        bind_fn: impl FnOnce(&mut Self, Local),
        incr_fn: impl FnOnce(&mut Self),
    ) {
        let head = self.new_block();
        self.terminate(Terminator::Goto(head));
        self.switch(head);
        let (body_bb, exit_bb, incr, after) = (
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
        );
        head_fn(self, body_bb, exit_bb);
        let entry = self.moved.clone();
        self.loops.push(LoopFrame {
            cont: incr,
            body_depth: self.scopes.len(),
            breaks: Vec::new(),
        });
        self.switch(body_bb);
        self.diverged = false;
        self.enter_scope();
        let var_l = Local(var.0);
        bind_fn(self, var_l);
        if self.locals[var_l.0].is_ref {
            self.scopes.last_mut().unwrap().refs.push(var_l);
        }
        for s in &body.stmts {
            self.stmt(s);
        }
        self.exit_scope();
        if !self.diverged {
            self.terminate(Terminator::Goto(incr));
        }
        self.switch(incr);
        incr_fn(self);
        self.terminate(Terminator::Goto(head));
        let frame = self.loops.pop().unwrap();
        let mut ends: Vec<(usize, BTreeSet<Local>, bool)> = frame
            .breaks
            .into_iter()
            .map(|(b, s)| (b, s, false))
            .collect();
        ends.push((exit_bb.0, entry.clone(), false));
        self.moved = entry;
        self.join(ends, after);
    }

    // ----- expressions ------------------------------------------------

    /// Lower a place expression rooted at a local.
    fn place(&mut self, e: &hir::Expr) -> Place {
        match &e.kind {
            EK::Local(l) => Place::local(Local(l.0)),
            EK::Field(b, i) => {
                let mut p = self.place(b);
                p.proj.push(Proj::Field(*i));
                p
            }
            EK::Index(b, i) => {
                let mut p = self.place(b);
                let idx = self.index_local(i);
                p.proj.push(Proj::Index(idx, self.loc(e.span)));
                p
            }
            _ => unreachable!("not a place"),
        }
    }

    fn index_local(&mut self, i: &hir::Expr) -> Local {
        let op = self.value(i);
        match op {
            Operand::Copy(Place { local, ref proj }) if proj.is_empty() => local,
            other => {
                let t = self.temp(Ty::Int);
                self.assign(Place::local(t), Rvalue::Use(other));
                t
            }
        }
    }

    /// A place readable for the rest of the statement. Copy values are
    /// snapshotted into a temporary so later side effects in the same
    /// statement cannot change what was read.
    fn read(&mut self, e: &hir::Expr) -> Place {
        if e.is_place() {
            let p = self.place(e);
            if self.is_copy(&e.ty) {
                let t = self.temp(e.ty.clone());
                self.assign(Place::local(t), Rvalue::Use(Operand::Copy(p)));
                return Place::local(t);
            }
            return p;
        }
        if let Some(p) = self.temp_projection(e) {
            return p;
        }
        match self.value(e) {
            Operand::Copy(p) | Operand::Move(p) => p,
            Operand::Ref(_) => unreachable!("value() never yields a reference"),
            Operand::Const(c) => {
                let t = self.live_temp(e.ty.clone());
                self.assign(Place::local(t), Rvalue::Use(Operand::Const(c)));
                Place::local(t)
            }
        }
    }

    /// Field/index access on a temporary base: `make().x`, `f()[0]`.
    fn temp_projection(&mut self, e: &hir::Expr) -> Option<Place> {
        let (base, proj) = match &e.kind {
            EK::Field(b, i) => (b, Some(*i)),
            EK::Index(b, _) => (b, None),
            _ => return None,
        };
        let mut p = self.read(base);
        match (proj, &e.kind) {
            (Some(i), _) => p.proj.push(Proj::Field(i)),
            (None, EK::Index(_, i)) => {
                let idx = self.index_local(i);
                p.proj.push(Proj::Index(idx, self.loc(e.span)));
            }
            _ => unreachable!(),
        }
        if self.is_copy(&e.ty) {
            let t = self.temp(e.ty.clone());
            self.assign(Place::local(t), Rvalue::Use(Operand::Copy(p)));
            return Some(Place::local(t));
        }
        Some(p)
    }

    /// Lower an expression to an operand holding an owned value (or a copy).
    fn value(&mut self, e: &hir::Expr) -> Operand {
        match &e.kind {
            EK::Int(v) => Operand::Const(Const::Int(*v)),
            EK::Float(v) => Operand::Const(Const::Float(*v)),
            EK::Bool(v) => Operand::Const(Const::Bool(*v)),
            EK::Str(s) => Operand::Const(Const::Str(s.clone())),
            EK::Local(_) | EK::Field(..) | EK::Index(..) if e.is_place() => {
                assert!(
                    self.is_copy(&e.ty),
                    "ownership checker admitted an implicit move"
                );
                let p = self.place(e);
                let t = self.temp(e.ty.clone());
                self.assign(Place::local(t), Rvalue::Use(Operand::Copy(p)));
                Operand::Copy(Place::local(t))
            }
            EK::Field(..) | EK::Index(..) => {
                let p = self.temp_projection(e).unwrap();
                Operand::Copy(p)
            }
            EK::Take(p) => {
                let src = self.place(p);
                let t = self.live_temp(e.ty.clone());
                self.moved.insert(src.local);
                self.assign(Place::local(t), Rvalue::Use(Operand::Move(src)));
                self.owned_operand(t, &e.ty)
            }
            EK::Try(x) => {
                let r = self.lower_try(x, e);
                r.expect("try of Result[Void] used as a value")
            }
            EK::Binary(BinOp::And | BinOp::Or, ..) => self.short_circuit(e),
            _ => {
                let r = self.rvalue(e);
                let t = self.live_temp(e.ty.clone());
                self.assign(Place::local(t), r);
                self.owned_operand(t, &e.ty)
            }
        }
    }

    fn owned_operand(&self, t: Local, ty: &Ty) -> Operand {
        if self.is_copy(ty) {
            Operand::Copy(Place::local(t))
        } else {
            Operand::Move(Place::local(t))
        }
    }

    fn short_circuit(&mut self, e: &hir::Expr) -> Operand {
        let EK::Binary(op, a, b) = &e.kind else {
            unreachable!()
        };
        let la = self.value(a);
        let r = self.temp(Ty::Bool);
        let (rhs_bb, short_bb, join_bb) = (self.new_block(), self.new_block(), self.new_block());
        if *op == BinOp::And {
            self.terminate(Terminator::Branch(la, rhs_bb, short_bb));
        } else {
            self.terminate(Terminator::Branch(la, short_bb, rhs_bb));
        }
        self.switch(short_bb);
        self.assign(
            Place::local(r),
            Rvalue::Use(Operand::Const(Const::Bool(*op == BinOp::Or))),
        );
        self.terminate(Terminator::Goto(join_bb));
        self.switch(rhs_bb);
        self.push_temps();
        let lb = self.value(b);
        self.assign(Place::local(r), Rvalue::Use(lb));
        self.pop_temps();
        self.terminate(Terminator::Goto(join_bb));
        self.switch(join_bb);
        Operand::Copy(Place::local(r))
    }

    /// `try x`: returns the success payload operand (None for Void).
    fn lower_try(&mut self, x: &hir::Expr, whole: &hir::Expr) -> Option<Operand> {
        let op = self.value(x);
        let rp = match op {
            Operand::Move(p) => p,
            _ => unreachable!("Result values are owned"),
        };
        let c = self.temp(Ty::Bool);
        self.assign(Place::local(c), Rvalue::IsOk(rp.clone()));
        let (ok_bb, err_bb) = (self.new_block(), self.new_block());
        self.terminate(Terminator::Branch(
            Operand::Copy(Place::local(c)),
            ok_bb,
            err_bb,
        ));
        // Both paths move the payload out of the Result temporary.
        self.moved.insert(rp.local);
        self.switch(err_bb);
        let e = self.temp(Ty::String);
        self.assign(Place::local(e), Rvalue::ResultTakeErr(rp.clone()));
        let rl = self.ret_local.expect("try outside Result function");
        self.assign(
            Place::local(rl),
            Rvalue::ResultFail(self.f.ret.clone(), Operand::Move(Place::local(e))),
        );
        self.drop_everything();
        self.terminate(Terminator::Return);
        self.switch(ok_bb);
        if whole.ty == Ty::Void {
            return None;
        }
        let v = self.live_temp(whole.ty.clone());
        self.assign(Place::local(v), Rvalue::ResultTakeOk(rp));
        Some(self.owned_operand(v, &whole.ty))
    }

    /// Lower a Void expression evaluated for its effect.
    fn effect(&mut self, e: &hir::Expr) {
        match &e.kind {
            EK::Builtin(hir::Builtin::Print, args) => {
                let p = self.read(&args[0]);
                self.emit(Stmt::Print(p));
            }
            EK::Builtin(hir::Builtin::Assert, args) => {
                let c = self.value(&args[0]);
                let loc = self.loc(e.span);
                self.emit(Stmt::Assert(c, loc));
            }
            EK::Builtin(hir::Builtin::AssertEq, args) => {
                let a = self.read(&args[0]);
                let b = self.read(&args[1]);
                let loc = self.loc(e.span);
                self.emit(Stmt::AssertEq(a, b, loc));
            }
            EK::Builtin(hir::Builtin::Append, args) => {
                let EK::Borrow(_, lp) = &args[0].kind else {
                    unreachable!()
                };
                let p = self.place(lp);
                let v = self.value(&args[1]);
                self.mark_moved(&v);
                self.emit(Stmt::Append(p, v));
            }
            EK::Try(x) => {
                self.lower_try(x, e);
            }
            _ => {
                let r = self.rvalue(e);
                self.emit(Stmt::Eval(r));
            }
        }
    }

    /// Lower a non-place, non-literal expression to an rvalue. Operands that
    /// the rvalue consumes are marked moved here (the caller emits the
    /// rvalue immediately).
    fn rvalue(&mut self, e: &hir::Expr) -> Rvalue {
        let loc = self.loc(e.span);
        match &e.kind {
            EK::Unary(UnOp::Neg, x) => {
                let o = self.value(x);
                if x.ty == Ty::Float {
                    Rvalue::FloatNeg(o)
                } else {
                    Rvalue::CheckedNeg(o, loc)
                }
            }
            EK::Unary(UnOp::Not, x) => Rvalue::Not(self.value(x)),
            EK::Binary(op, a, b) => {
                let ty = a.ty.clone();
                match op {
                    BinOp::Add if ty == Ty::String => {
                        let pa = self.read(a);
                        let pb = self.read(b);
                        Rvalue::Concat(pa, pb)
                    }
                    BinOp::Eq | BinOp::Ne if matches!(ty, Ty::String | Ty::Record(_)) => {
                        let pa = self.read(a);
                        let pb = self.read(b);
                        Rvalue::Equal {
                            ty,
                            a: pa,
                            b: pb,
                            negate: *op == BinOp::Ne,
                        }
                    }
                    _ if op.is_comparison() => {
                        let oa = self.value(a);
                        let ob = self.value(b);
                        Rvalue::Compare(*op, oa, ob)
                    }
                    _ => {
                        let oa = self.value(a);
                        let ob = self.value(b);
                        if ty == Ty::Float {
                            let fop = match op {
                                BinOp::Add => FloatOp::Add,
                                BinOp::Sub => FloatOp::Sub,
                                BinOp::Mul => FloatOp::Mul,
                                _ => FloatOp::Div,
                            };
                            Rvalue::Float(fop, oa, ob)
                        } else {
                            let iop = match op {
                                BinOp::Add => IntOp::Add,
                                BinOp::Sub => IntOp::Sub,
                                BinOp::Mul => IntOp::Mul,
                                BinOp::Div => IntOp::Div,
                                _ => IntOp::Rem,
                            };
                            Rvalue::CheckedInt(iop, oa, ob, loc)
                        }
                    }
                }
            }
            EK::Call(fid, args) => {
                let mut ops = Vec::new();
                for (pass, a) in args {
                    let op = match pass {
                        PassMode::Copy | PassMode::Move => self.value(a),
                        PassMode::Borrow { .. } => {
                            let p = match &a.kind {
                                EK::Borrow(_, p) => self.place(p),
                                _ if a.is_place() => self.place(a),
                                _ => self.read(a),
                            };
                            let r = self.new_local(a.ty.clone(), true, None);
                            self.emit(Stmt::BindRef(r, p));
                            Operand::Ref(r)
                        }
                    };
                    ops.push(op);
                }
                for o in &ops {
                    self.mark_moved(o);
                }
                Rvalue::Call(fid.0, ops)
            }
            EK::Builtin(b, args) => match b {
                hir::Builtin::Len => Rvalue::Len(self.read(&args[0])),
                hir::Builtin::Clone => Rvalue::Clone(self.read(&args[0])),
                hir::Builtin::IsOk => Rvalue::IsOk(self.read(&args[0])),
                hir::Builtin::Unwrap | hir::Builtin::UnwrapError => {
                    let o = self.value(&args[0]);
                    self.mark_moved(&o);
                    if *b == hir::Builtin::Unwrap {
                        Rvalue::Unwrap(o, loc)
                    } else {
                        Rvalue::UnwrapError(o, loc)
                    }
                }
                hir::Builtin::IntToFloat => Rvalue::IntToFloat(self.value(&args[0])),
                hir::Builtin::FloatToInt => Rvalue::FloatToInt(self.value(&args[0]), loc),
                hir::Builtin::IntToByte => Rvalue::IntToByte(self.value(&args[0]), loc),
                hir::Builtin::ByteToInt => Rvalue::ByteToInt(self.value(&args[0])),
                hir::Builtin::Print
                | hir::Builtin::Assert
                | hir::Builtin::AssertEq
                | hir::Builtin::Append => {
                    unreachable!("Void builtin in value position")
                }
            },
            EK::Construct(rid, fields) => {
                let mut ops: Vec<Option<Operand>> =
                    vec![None; self.prog.records[rid.0].fields.len()];
                for (i, v) in fields {
                    ops[*i] = Some(self.value(v));
                }
                let ops: Vec<Operand> = ops
                    .into_iter()
                    .map(|o| o.expect("all fields initialized"))
                    .collect();
                for o in &ops {
                    self.mark_moved(o);
                }
                Rvalue::Construct(*rid, ops)
            }
            EK::List(elems) => {
                let elem_ty = match &e.ty {
                    Ty::List(t) => (**t).clone(),
                    _ => unreachable!(),
                };
                let ops: Vec<Operand> = elems.iter().map(|x| self.value(x)).collect();
                for o in &ops {
                    self.mark_moved(o);
                }
                Rvalue::ListLit(elem_ty, ops)
            }
            EK::BufferNew(x) => Rvalue::BufferNew(self.value(x), loc),
            EK::ResultOk(x) => {
                let o = x.as_ref().map(|x| self.value(x));
                if let Some(o) = &o {
                    self.mark_moved(o);
                }
                Rvalue::ResultOk(e.ty.clone(), o)
            }
            EK::ResultFail(x) => {
                let o = self.value(x);
                self.mark_moved(&o);
                Rvalue::ResultFail(e.ty.clone(), o)
            }
            EK::Try(_)
            | EK::Take(_)
            | EK::Local(_)
            | EK::Field(..)
            | EK::Index(..)
            | EK::Int(_)
            | EK::Float(_)
            | EK::Bool(_)
            | EK::Str(_) => {
                let o = self.value(e);
                Rvalue::Use(o)
            }
            EK::Borrow(..) => unreachable!("borrow outside let/argument position"),
        }
    }
}
