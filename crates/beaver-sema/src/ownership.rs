//! Ownership, move and lexical-borrow checking over the typed HIR.
//!
//! The checker walks each function body in evaluation order, tracking:
//!
//! * `moved`: the set of owned locals whose value has been transferred with
//!   `take` on the current path;
//! * `borrows`: active borrows, each tied to a *root* local (borrowing a field
//!   or element borrows the whole owner). Named borrows (`let r = view x`,
//!   `for item in list`) last until their lexical scope closes; temporary
//!   borrows created for call arguments end at the end of the statement.
//!
//! Control-flow joins (`if`/`else`, loop exits) accept differing move states
//! only for locals that are never mentioned again (the MIR then drops them on
//! the paths where they are still owned); otherwise the program is rejected.
//! Loop back-edges must not have moved any local declared outside the loop.

use crate::hir::*;
use crate::types::{RecordDef, Ty};
use beaver_source::{Diagnostic, SourceMap, Span};
use std::collections::BTreeSet;

pub fn check(
    functions: &[Function],
    records: &[RecordDef],
    sources: &SourceMap,
) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    for f in functions {
        let mut c = Checker {
            f,
            records,
            sources,
            moved: BTreeSet::new(),
            diverged: false,
            borrows: Vec::new(),
            depth: 1,
            local_depth: vec![0; f.locals.len()],
            scope_locals: vec![Vec::new()],
            loops: Vec::new(),
            move_sites: vec![None; f.locals.len()],
            errors: Vec::new(),
        };
        for p in &f.params {
            c.declare(p.local);
        }
        let cont = BTreeSet::new();
        c.stmts(&f.body.stmts, &cont);
        // Deduplicate identical reports.
        let mut seen = BTreeSet::new();
        for d in c.errors {
            let key = (
                d.code,
                d.span.map(|s| (s.file.0, s.start, s.end)),
                d.message.clone(),
            );
            if seen.insert(key) {
                errors.push(d);
            }
        }
    }
    errors
}

struct Borrow {
    root: LocalId,
    mutable: bool,
    /// Scope depth that owns the borrow; `None` = statement temporary.
    depth: Option<usize>,
    span: Span,
    borrower: Option<String>,
}

struct LoopCtx {
    depth: usize,
    entry: BTreeSet<LocalId>,
    breaks: Vec<BTreeSet<LocalId>>,
}

#[derive(Clone, Copy)]
enum Ctx {
    Let,
    Assign,
    Return,
    Store(&'static str),
    TakeArg,
    LoopTemp,
    Unwrap,
    Try,
}

impl Ctx {
    fn persists(self) -> bool {
        matches!(self, Ctx::Return | Ctx::Store(_))
    }
}

struct Checker<'a> {
    f: &'a Function,
    records: &'a [RecordDef],
    sources: &'a SourceMap,
    moved: BTreeSet<LocalId>,
    diverged: bool,
    borrows: Vec<Borrow>,
    depth: usize,
    local_depth: Vec<usize>,
    scope_locals: Vec<Vec<LocalId>>,
    loops: Vec<LoopCtx>,
    move_sites: Vec<Option<Span>>,
    errors: Vec<Diagnostic>,
}

impl<'a> Checker<'a> {
    fn name(&self, l: LocalId) -> &str {
        &self.f.locals[l.0].name
    }

    fn line(&self, span: Span) -> usize {
        self.sources.location(span).1
    }

    fn declare(&mut self, l: LocalId) {
        self.local_depth[l.0] = self.depth;
        self.scope_locals.last_mut().unwrap().push(l);
    }

    fn enter_scope(&mut self) {
        self.depth += 1;
        self.scope_locals.push(Vec::new());
    }

    fn exit_scope(&mut self) {
        let d = self.depth;
        self.borrows
            .retain(|b| !matches!(b.depth, Some(bd) if bd >= d));
        for l in self.scope_locals.pop().unwrap() {
            self.moved.remove(&l);
        }
        self.depth -= 1;
    }

    fn end_statement(&mut self) {
        self.borrows.retain(|b| b.depth.is_some());
    }

    fn block(&mut self, b: &Block, cont: &BTreeSet<LocalId>) {
        self.enter_scope();
        self.stmts(&b.stmts, cont);
        self.exit_scope();
    }

    fn stmts(&mut self, stmts: &[Stmt], cont: &BTreeSet<LocalId>) {
        let mut suffix = vec![BTreeSet::new(); stmts.len()];
        let mut acc = cont.clone();
        for i in (0..stmts.len()).rev() {
            suffix[i] = acc.clone();
            mentions_stmt(&stmts[i], &mut acc);
        }
        for (s, c) in stmts.iter().zip(suffix.iter()) {
            self.stmt(s, c);
        }
    }

    fn stmt(&mut self, s: &Stmt, cont: &BTreeSet<LocalId>) {
        match &s.kind {
            StmtKind::Let { local, init } => {
                if let ExprKind::Borrow(kind, place) = &init.kind {
                    let name = self.name(*local).to_string();
                    self.borrow_place(
                        place,
                        *kind == BorrowKind::Edit,
                        Some(self.depth),
                        init.span,
                        Some(name),
                    );
                } else {
                    self.consume(init, Ctx::Let);
                }
                self.end_statement();
                self.declare(*local);
            }
            StmtKind::Assign { target, op, value } => {
                if *op != AssignOp::Set {
                    self.read_place(target);
                }
                self.consume(value, Ctx::Assign);
                if *op == AssignOp::Set {
                    self.place_indices(target);
                }
                self.end_statement();
                self.write_place(target);
            }
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.consume(e, Ctx::Return);
                }
                self.end_statement();
                self.diverged = true;
            }
            StmtKind::If { cond, then, els } => {
                self.expr(cond);
                self.end_statement();
                let before = (self.moved.clone(), self.diverged);
                self.block(then, cont);
                let a = (
                    std::mem::replace(&mut self.moved, before.0.clone()),
                    self.diverged,
                );
                self.diverged = before.1;
                if let Some(e) = els {
                    self.block(e, cont);
                }
                let b = (self.moved.clone(), self.diverged);
                self.join(vec![a, b], cont, s.span);
            }
            StmtKind::While { cond, body } => {
                let mut cont_body = cont.clone();
                cont_body.extend(next_iteration_mentions(body));
                mentions_expr(cond, &mut cont_body);
                self.loops.push(LoopCtx {
                    depth: self.depth,
                    entry: self.moved.clone(),
                    breaks: Vec::new(),
                });
                self.expr(cond);
                self.end_statement();
                let after_cond = self.moved.clone();
                self.block(body, &cont_body);
                if !self.diverged {
                    self.back_edge();
                }
                let lp = self.loops.pop().unwrap();
                let mut exits: Vec<(BTreeSet<LocalId>, bool)> =
                    lp.breaks.into_iter().map(|b| (b, false)).collect();
                if !matches!(cond.kind, ExprKind::Bool(true)) {
                    exits.push((after_cond, false));
                }
                self.join(exits, cont, s.span);
            }
            StmtKind::ForRange {
                var,
                start,
                end,
                body,
            } => {
                self.expr(start);
                self.expr(end);
                self.end_statement();
                self.loop_body(*var, body, cont, s.span);
            }
            StmtKind::ForList { var, iter, body } => {
                self.enter_scope();
                if iter.is_place() {
                    self.borrow_place(
                        iter,
                        false,
                        Some(self.depth),
                        iter.span,
                        Some(format!("for-loop over `{}`", self.place_name(iter))),
                    );
                } else {
                    self.consume(iter, Ctx::LoopTemp);
                }
                self.end_statement();
                self.loop_body(*var, body, cont, s.span);
                self.exit_scope();
            }
            StmtKind::Break => {
                let lp = self.loops.last().unwrap();
                let d = lp.depth;
                let state: BTreeSet<LocalId> = self
                    .moved
                    .iter()
                    .copied()
                    .filter(|l| self.local_depth[l.0] <= d)
                    .collect();
                self.loops.last_mut().unwrap().breaks.push(state);
                self.diverged = true;
            }
            StmtKind::Continue => {
                self.back_edge();
                self.diverged = true;
            }
            StmtKind::Scope(b) => self.block(b, cont),
            StmtKind::Expr(e) => {
                if e.is_place() {
                    self.read_place(e);
                } else {
                    self.expr(e);
                }
                self.end_statement();
            }
        }
    }

    fn loop_body(&mut self, var: LocalId, body: &Block, cont: &BTreeSet<LocalId>, span: Span) {
        let mut cont_body = cont.clone();
        cont_body.extend(next_iteration_mentions(body));
        let entry = self.moved.clone();
        self.loops.push(LoopCtx {
            depth: self.depth,
            entry: entry.clone(),
            breaks: Vec::new(),
        });
        // The loop variable is fresh in every iteration: declare it inside.
        self.enter_scope();
        self.declare(var);
        self.stmts(&body.stmts, &cont_body);
        self.exit_scope();
        if !self.diverged {
            self.back_edge();
        }
        let lp = self.loops.pop().unwrap();
        let mut exits: Vec<(BTreeSet<LocalId>, bool)> =
            lp.breaks.into_iter().map(|b| (b, false)).collect();
        exits.push((entry, false));
        self.join(exits, cont, span);
    }

    fn back_edge(&mut self) {
        let lp = self.loops.last().unwrap();
        let bad: Vec<LocalId> = self
            .moved
            .iter()
            .copied()
            .filter(|l| self.local_depth[l.0] <= lp.depth && !lp.entry.contains(l))
            .collect();
        for l in bad {
            let span = self.move_sites[l.0].unwrap_or(self.f.span);
            let name = self.name(l).to_string();
            self.errors.push(
                Diagnostic::error(
                    "B3010",
                    format!("`{name}` is moved inside a loop, so the next iteration would use a moved value"),
                    span,
                )
                .with_label("moved here")
                .with_help(format!("move `{name}` only on a path that leaves the loop (`break`/`return`), or `clone` it")),
            );
            // Avoid repeating the report at every back edge.
            self.loops.last_mut().unwrap().entry.insert(l);
        }
    }

    fn join(
        &mut self,
        incoming: Vec<(BTreeSet<LocalId>, bool)>,
        cont: &BTreeSet<LocalId>,
        span: Span,
    ) {
        let live: Vec<BTreeSet<LocalId>> = incoming
            .into_iter()
            .filter(|(_, d)| !d)
            .map(|(s, _)| s)
            .collect();
        if live.is_empty() {
            self.diverged = true;
            return;
        }
        let mut union = BTreeSet::new();
        for s in &live {
            union.extend(s.iter().copied());
        }
        for v in &union {
            let everywhere = live.iter().all(|s| s.contains(v));
            if !everywhere && cont.contains(v) {
                let name = self.name(*v).to_string();
                let mut d = Diagnostic::error(
                    "B3009",
                    format!("`{name}` is moved on some paths but not on others, and is used afterwards"),
                    span,
                )
                .with_note("Beaver 0.1 requires a value's ownership state to be identical where control-flow paths meet");
                if let Some(ms) = self.move_sites[v.0] {
                    d = d.with_note(format!("`{name}` is moved on line {}", self.line(ms)));
                }
                self.errors.push(d.with_help(
                    "move it on every path, or clone it in the branch that consumes it",
                ));
            }
        }
        self.moved = union;
        self.diverged = false;
    }

    fn place_name(&self, p: &Expr) -> String {
        match &p.kind {
            ExprKind::Local(l) => self.name(*l).to_string(),
            ExprKind::Field(b, i) => {
                let base = self.place_name(b);
                match &b.ty {
                    Ty::Record(r) => format!("{base}.{}", self.records[r.0].fields[*i].0),
                    _ => base,
                }
            }
            ExprKind::Index(b, _) => format!("{}[...]", self.place_name(b)),
            _ => "value".into(),
        }
    }

    // ----- place operations -------------------------------------------

    fn use_local(&mut self, l: LocalId, span: Span) {
        if self.moved.contains(&l) {
            self.report_moved(l, span, "use");
            return;
        }
        if let Some(b) = self.borrows.iter().find(|b| b.root == l && b.mutable) {
            let line = self.line(b.span);
            let by = b.borrower.clone();
            let name = self.name(l).to_string();
            let mut d = Diagnostic::error(
                "B3003",
                format!("cannot use `{name}` while it is borrowed by `edit`"),
                span,
            )
            .with_note(format!("the exclusive borrow starts on line {line}"));
            if let Some(by) = by {
                d = d.with_help(format!(
                    "wrap `{by}` in a `scope:` block to end the borrow earlier"
                ));
            }
            self.errors.push(d);
        }
    }

    fn report_moved(&mut self, l: LocalId, span: Span, what: &str) {
        let name = self.name(l).to_string();
        let mut d = Diagnostic::error("B3001", format!("{what} of moved value `{name}`"), span)
            .with_label("value used after its ownership was transferred");
        if let Some(ms) = self.move_sites[l.0] {
            d = d.with_note(format!(
                "`{name}` was moved with `take` on line {}",
                self.line(ms)
            ));
        }
        d = d.with_help(format!(
            "use `clone({name})` before moving if both copies are needed"
        ));
        self.errors.push(d);
    }

    /// Evaluate the index sub-expressions of a place, left to right.
    fn place_indices(&mut self, p: &Expr) {
        match &p.kind {
            ExprKind::Field(b, _) => self.place_indices(b),
            ExprKind::Index(b, i) => {
                self.place_indices(b);
                self.expr(i);
            }
            _ => {}
        }
    }

    fn read_place(&mut self, p: &Expr) {
        let root = p.place_root().unwrap();
        self.use_local(root, p.span);
        self.place_indices(p);
    }

    fn write_place(&mut self, p: &Expr) {
        let root = p.place_root().unwrap();
        if self.moved.contains(&root) {
            let name = self.name(root).to_string();
            self.errors.push(
                Diagnostic::error("B3001", format!("cannot assign to `{name}` after it was moved"), p.span)
                    .with_note("re-initializing a moved variable is not supported in Beaver 0.1; declare a new variable"),
            );
            return;
        }
        if let Some(b) = self.borrows.iter().find(|b| b.root == root) {
            let line = self.line(b.span);
            let by = b.borrower.clone();
            let name = self.name(root).to_string();
            let mut d = Diagnostic::error(
                "B3014",
                format!("cannot assign to `{name}` while it is borrowed"),
                p.span,
            )
            .with_note(format!(
                "the borrow starts on line {line} and lasts until the end of its scope"
            ));
            if let Some(by) = by {
                d = d.with_help(format!(
                    "end the borrow `{by}` first by wrapping it in `scope:`"
                ));
            }
            self.errors.push(d);
        }
    }

    fn borrow_place(
        &mut self,
        p: &Expr,
        mutable: bool,
        depth: Option<usize>,
        span: Span,
        borrower: Option<String>,
    ) {
        let root = p.place_root().unwrap();
        self.place_indices(p);
        if self.moved.contains(&root) {
            self.report_moved(root, p.span, "borrow");
            return;
        }
        let info = &self.f.locals[root.0];
        let name = info.name.clone();
        if mutable {
            match info.kind {
                LocalKind::Owned { mutable: false } => {
                    let d = Diagnostic::error(
                        "B3004",
                        format!("cannot `edit` immutable `{name}`"),
                        span,
                    );
                    let d = if info.is_param {
                        d.with_note("parameters are immutable; declare the parameter as `edit` to mutate the caller's value")
                    } else {
                        d.with_help(format!("declare it with `var {name}` to allow mutation"))
                    };
                    self.errors.push(d);
                    return;
                }
                LocalKind::Ref { mutable: false } => {
                    self.errors.push(
                        Diagnostic::error(
                            "B3004",
                            format!("cannot `edit` through read-only reference `{name}`"),
                            span,
                        )
                        .with_note("`view` references and plain non-Copy parameters are read-only"),
                    );
                    return;
                }
                _ => {}
            }
            if let Some(b) = self.borrows.iter().find(|b| b.root == root) {
                let line = self.line(b.span);
                let kind = if b.mutable { "edit" } else { "view" };
                let by = b.borrower.clone();
                let mut d = Diagnostic::error(
                    "B3002",
                    format!("cannot edit `{name}` while it is borrowed"),
                    span,
                )
                .with_label("conflicting exclusive borrow")
                .with_note(format!(
                    "`{name}` already has an active `{kind}` borrow from line {line}; borrows of any field borrow the whole value"
                ));
                d = match by {
                    Some(by) if b.depth.is_some() => d
                        .with_note("the earlier borrow remains active until its lexical scope ends")
                        .with_help(format!(
                            "wrap `{by}` in `scope:` to shorten the borrow lifetime"
                        )),
                    _ => d.with_note(
                        "borrows taken for call arguments last until the end of the statement",
                    ),
                };
                self.errors.push(d);
                return;
            }
        } else if let Some(b) = self.borrows.iter().find(|b| b.root == root && b.mutable) {
            let line = self.line(b.span);
            let by = b.borrower.clone();
            let mut d = Diagnostic::error(
                "B3002",
                format!("cannot view `{name}` while it is being edited"),
                span,
            )
            .with_label("conflicting shared borrow")
            .with_note(format!("the exclusive `edit` borrow starts on line {line}"));
            if let Some(by) = by {
                d = d.with_help(format!(
                    "wrap `{by}` in `scope:` to end the edit borrow first"
                ));
            }
            self.errors.push(d);
            return;
        }
        self.borrows.push(Borrow {
            root,
            mutable,
            depth,
            span,
            borrower,
        });
    }

    fn take(&mut self, p: &Expr, span: Span) {
        let ExprKind::Local(l) = p.kind else {
            let what = if matches!(p.kind, ExprKind::Index(..)) {
                "an element out of a list"
            } else {
                "a field out of its record"
            };
            self.place_indices(p);
            self.errors.push(
                Diagnostic::error("B3006", format!("cannot `take` {what}"), span)
                    .with_note("partial moves are not supported in Beaver 0.1")
                    .with_help(format!(
                        "use `clone({})` to get an owned copy",
                        self.place_name(p)
                    )),
            );
            return;
        };
        let info = &self.f.locals[l.0];
        let name = info.name.clone();
        if info.is_ref() {
            self.errors.push(
                Diagnostic::error(
                    "B3005",
                    format!("cannot `take` borrowed value `{name}`"),
                    span,
                )
                .with_note(format!(
                    "`{name}` is a reference; it does not own its value"
                ))
                .with_help(format!("use `clone({name})` to get an owned copy")),
            );
            return;
        }
        if self.moved.contains(&l) {
            self.report_moved(l, p.span, "move");
            return;
        }
        if let Some(b) = self.borrows.iter().find(|b| b.root == l) {
            let line = self.line(b.span);
            self.errors.push(
                Diagnostic::error(
                    "B3012",
                    format!("cannot `take` `{name}` while it is borrowed"),
                    span,
                )
                .with_note(format!("the borrow starts on line {line}")),
            );
            return;
        }
        self.moved.insert(l);
        self.move_sites[l.0] = Some(span);
    }

    // ----- expressions -------------------------------------------------

    fn consume(&mut self, e: &Expr, ctx: Ctx) {
        match &e.kind {
            ExprKind::Take(p) => self.take(p, e.span),
            ExprKind::Borrow(..) => {
                let what = match ctx {
                    Ctx::Return => "returned".to_string(),
                    Ctx::Store(w) => format!("stored in a {w}"),
                    _ => "moved".to_string(),
                };
                self.errors.push(Diagnostic::error(
                    "B3008",
                    format!("a reference cannot be {what}"),
                    e.span,
                ));
            }
            _ if e.is_place() => {
                let root = e.place_root().unwrap();
                let info = self.f.locals[root.0].clone();
                let bare_ref = matches!(e.kind, ExprKind::Local(_)) && info.is_ref();
                if bare_ref && ctx.persists() {
                    let what = match ctx {
                        Ctx::Return => "returned from a function".to_string(),
                        Ctx::Store(w) => format!("stored in a {w}"),
                        _ => unreachable!(),
                    };
                    self.errors.push(
                        Diagnostic::error(
                            "B3008",
                            format!("reference `{}` cannot be {what}", info.name),
                            e.span,
                        )
                        .with_note("borrowed references must not outlive the statement or scope they belong to")
                        .with_help(format!("store an owned copy with `clone({})`", info.name)),
                    );
                    return;
                }
                if e.ty.is_copy(self.records) {
                    self.read_place(e);
                    return;
                }
                let tn = e.ty.display(self.records);
                let pname = self.place_name(e);
                let d = if info.is_ref() {
                    let code = if matches!(ctx, Ctx::Return) {
                        "B3008"
                    } else {
                        "B3005"
                    };
                    let msg = if matches!(ctx, Ctx::Return) {
                        format!("cannot return borrowed value `{pname}`")
                    } else {
                        format!("cannot move `{pname}` out of a reference")
                    };
                    Diagnostic::error(code, msg, e.span)
                        .with_note(format!(
                            "`{}` is a borrowed reference and does not own its {tn}",
                            info.name
                        ))
                        .with_help(format!("use `clone({pname})` to produce an owned value"))
                } else if !matches!(e.kind, ExprKind::Local(_)) {
                    Diagnostic::error(
                        "B3006",
                        format!("cannot move `{pname}` out of its owner"),
                        e.span,
                    )
                    .with_note(
                        "partial moves out of records and lists are not supported in Beaver 0.1",
                    )
                    .with_help(format!("use `clone({pname})`"))
                } else {
                    Diagnostic::error(
                        "B3007",
                        format!("`{pname}` ({tn}) is not Copy and cannot be moved implicitly"),
                        e.span,
                    )
                    .with_help(format!("write `take {pname}` to transfer ownership, or `clone({pname})` to duplicate it"))
                };
                // Still check the read for use-after-move.
                self.read_place(e);
                self.errors.push(d);
            }
            ExprKind::Field(b, _) | ExprKind::Index(b, _) if !e.ty.is_copy(self.records) => {
                self.expr(b);
                self.errors.push(
                    Diagnostic::error(
                        "B3006",
                        "cannot move a non-Copy part out of a temporary value",
                        e.span,
                    )
                    .with_help("bind the whole value to a variable first, or use `clone(...)`"),
                );
            }
            _ => self.expr(e),
        }
    }

    fn read_arg(&mut self, a: &Expr) {
        if a.is_place() {
            if a.ty.is_copy(self.records) {
                self.read_place(a);
            } else {
                self.borrow_place(a, false, None, a.span, None);
            }
        } else {
            self.expr(a);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
            ExprKind::Local(l) => self.use_local(*l, e.span),
            ExprKind::Field(b, _) | ExprKind::Index(b, _) => {
                if e.is_place() {
                    self.read_place(e);
                } else {
                    self.expr(b);
                    if let ExprKind::Index(_, i) = &e.kind {
                        self.expr(i);
                    }
                }
            }
            ExprKind::Unary(_, x) => self.expr(x),
            ExprKind::Binary(op, a, b) => {
                self.expr(a);
                if matches!(op, BinOp::And | BinOp::Or) {
                    let before = self.moved.clone();
                    self.expr(b);
                    if self.moved != before {
                        self.errors.push(
                            Diagnostic::error(
                                "B3015",
                                format!(
                                    "cannot move a value inside the right operand of `{}`",
                                    op.symbol()
                                ),
                                b.span,
                            )
                            .with_note("that operand is evaluated only conditionally"),
                        );
                        self.moved = before;
                    }
                } else {
                    self.expr(b);
                }
            }
            ExprKind::Borrow(kind, p) => {
                self.borrow_place(p, *kind == BorrowKind::Edit, None, e.span, None)
            }
            ExprKind::Take(p) => self.take(p, e.span),
            ExprKind::Try(x) => self.consume(x, Ctx::Try),
            ExprKind::Call(_, args) => {
                for (pass, a) in args {
                    match pass {
                        PassMode::Copy => {
                            if a.is_place() {
                                self.read_place(a);
                            } else {
                                self.expr(a);
                            }
                        }
                        PassMode::Move => self.consume(a, Ctx::TakeArg),
                        PassMode::Borrow { mutable } => match &a.kind {
                            ExprKind::Borrow(k, p) => {
                                self.borrow_place(p, *k == BorrowKind::Edit, None, a.span, None)
                            }
                            _ if a.is_place() => self.borrow_place(a, *mutable, None, a.span, None),
                            _ => self.expr(a),
                        },
                    }
                }
            }
            ExprKind::Builtin(b, args) => match b {
                Builtin::Append => {
                    if let ExprKind::Borrow(_, p) = &args[0].kind {
                        self.borrow_place(p, true, None, args[0].span, None);
                    }
                    self.consume(&args[1], Ctx::Store("list"));
                }
                Builtin::Unwrap | Builtin::UnwrapError => self.consume(&args[0], Ctx::Unwrap),
                _ => {
                    for a in args {
                        self.read_arg(a);
                    }
                }
            },
            ExprKind::Construct(_, fields) => {
                for (_, v) in fields {
                    self.consume(v, Ctx::Store("record"));
                }
            }
            ExprKind::List(elems) => {
                for v in elems {
                    self.consume(v, Ctx::Store("list"));
                }
            }
            ExprKind::BufferNew(x) => self.expr(x),
            ExprKind::ResultOk(x) => {
                if let Some(x) = x {
                    self.consume(x, Ctx::Store("Result"));
                }
            }
            ExprKind::ResultFail(x) => self.consume(x, Ctx::Store("Result")),
        }
    }
}

// ----- mention sets (used for liveness at joins) ----------------------

/// Locals mentioned by a later iteration of a loop body. Locals declared
/// inside the body are fresh in every iteration and therefore excluded.
fn next_iteration_mentions(body: &Block) -> BTreeSet<LocalId> {
    let mut m = BTreeSet::new();
    mentions_block(body, &mut m);
    let mut declared = BTreeSet::new();
    declared_in_block(body, &mut declared);
    m.difference(&declared).copied().collect()
}

fn declared_in_block(b: &Block, out: &mut BTreeSet<LocalId>) {
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Let { local, .. } => {
                out.insert(*local);
            }
            StmtKind::If { then, els, .. } => {
                declared_in_block(then, out);
                if let Some(e) = els {
                    declared_in_block(e, out);
                }
            }
            StmtKind::While { body, .. } | StmtKind::Scope(body) => declared_in_block(body, out),
            StmtKind::ForRange { var, body, .. } | StmtKind::ForList { var, body, .. } => {
                out.insert(*var);
                declared_in_block(body, out);
            }
            _ => {}
        }
    }
}

pub fn mentions_block(b: &Block, out: &mut BTreeSet<LocalId>) {
    for s in &b.stmts {
        mentions_stmt(s, out);
    }
}

pub fn mentions_stmt(s: &Stmt, out: &mut BTreeSet<LocalId>) {
    match &s.kind {
        StmtKind::Let { init, .. } => mentions_expr(init, out),
        StmtKind::Assign { target, value, .. } => {
            mentions_expr(target, out);
            mentions_expr(value, out);
        }
        StmtKind::Return(e) => {
            if let Some(e) = e {
                mentions_expr(e, out);
            }
        }
        StmtKind::If { cond, then, els } => {
            mentions_expr(cond, out);
            mentions_block(then, out);
            if let Some(e) = els {
                mentions_block(e, out);
            }
        }
        StmtKind::While { cond, body } => {
            mentions_expr(cond, out);
            mentions_block(body, out);
        }
        StmtKind::ForRange {
            start, end, body, ..
        } => {
            mentions_expr(start, out);
            mentions_expr(end, out);
            mentions_block(body, out);
        }
        StmtKind::ForList { iter, body, .. } => {
            mentions_expr(iter, out);
            mentions_block(body, out);
        }
        StmtKind::Scope(b) => mentions_block(b, out),
        StmtKind::Break | StmtKind::Continue => {}
        StmtKind::Expr(e) => mentions_expr(e, out),
    }
}

pub fn mentions_expr(e: &Expr, out: &mut BTreeSet<LocalId>) {
    match &e.kind {
        ExprKind::Local(l) => {
            out.insert(*l);
        }
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
        ExprKind::Field(b, _)
        | ExprKind::Unary(_, b)
        | ExprKind::Borrow(_, b)
        | ExprKind::Take(b)
        | ExprKind::Try(b)
        | ExprKind::BufferNew(b)
        | ExprKind::ResultFail(b) => mentions_expr(b, out),
        ExprKind::Index(a, b) | ExprKind::Binary(_, a, b) => {
            mentions_expr(a, out);
            mentions_expr(b, out);
        }
        ExprKind::Call(_, args) => {
            for (_, a) in args {
                mentions_expr(a, out);
            }
        }
        ExprKind::Builtin(_, args) | ExprKind::List(args) => {
            for a in args {
                mentions_expr(a, out);
            }
        }
        ExprKind::Construct(_, fields) => {
            for (_, v) in fields {
                mentions_expr(v, out);
            }
        }
        ExprKind::ResultOk(x) => {
            if let Some(x) = x {
                mentions_expr(x, out);
            }
        }
    }
}
