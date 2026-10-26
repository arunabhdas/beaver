//! Name resolution and type checking: AST -> typed HIR.

use crate::hir::{self, Builtin, FnId, LocalId, LocalInfo, LocalKind, PassMode, TestCase};
use crate::load::{Loaded, ModKey, ProgramInput, ProgramKind};
use crate::types::{RecordDef, RecordId, Ty};
use beaver_source::{Diagnostic, Span};
use beaver_syntax::ast::{self, AssignOp, BinOp, BorrowKind, ExprKind as AK, ParamMode, UnOp};
use std::collections::HashMap;

pub struct Typed {
    pub records: Vec<RecordDef>,
    pub functions: Vec<hir::Function>,
    pub entry: Option<FnId>,
    pub tests: Vec<TestCase>,
}

const BUILTIN_FNS: &[&str] = &[
    "print",
    "assert",
    "assert_eq",
    "len",
    "append",
    "clone",
    "ok",
    "fail",
    "is_ok",
    "unwrap",
    "unwrap_error",
    "range",
];
const BUILTIN_TYPES: &[&str] = &[
    "Int", "Float", "Bool", "Byte", "String", "Void", "List", "Buffer", "Result",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Fn(usize),
    Record(usize),
    Const(usize),
}

#[derive(Clone, Debug)]
enum ConstVal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

struct ConstInfo<'a> {
    module: usize,
    public: bool,
    decl: &'a ast::ConstDecl,
    ty: Ty,
    value: Option<ConstVal>,
    /// 0 = unevaluated, 1 = evaluating, 2 = done
    state: u8,
}

struct SigParam {
    name: ast::Ident,
    mode: ParamMode,
    ty: Ty,
}

struct Sig<'a> {
    name: String,
    module: usize,
    public: bool,
    params: Vec<SigParam>,
    ret: Ty,
    span: Span,
    decl: Option<&'a ast::FnDecl>,
    test: Option<&'a ast::TestDecl>,
    mangled: String,
}

pub fn mangle_len(s: &str) -> String {
    format!("{}{}", s.len(), s)
}

fn module_mangle(pkg: &str, key: &ModKey) -> String {
    match key {
        ModKey::Src(segs) => format!(
            "bvr_{}{}",
            mangle_len(pkg),
            segs.iter().map(|s| mangle_len(s)).collect::<String>()
        ),
        ModKey::Test(stem) => format!("bvt_{}{}", mangle_len(pkg), mangle_len(stem)),
    }
}

pub fn check(loaded: &Loaded, input: &ProgramInput) -> Result<Typed, Vec<Diagnostic>> {
    let mut tc = Tc {
        loaded,
        input,
        items: vec![HashMap::new(); loaded.modules.len()],
        imports: vec![HashMap::new(); loaded.modules.len()],
        records: Vec::new(),
        record_decls: Vec::new(),
        consts: Vec::new(),
        sigs: Vec::new(),
        errors: Vec::new(),
    };
    tc.collect();
    tc.resolve_records();
    tc.eval_consts();
    tc.resolve_sigs();
    let functions = tc.check_bodies();
    let entry = tc.check_main();
    let tests = tc.collect_tests();
    if tc.errors.is_empty() {
        Ok(Typed {
            records: tc.records,
            functions,
            entry,
            tests,
        })
    } else {
        Err(tc.errors)
    }
}

struct Tc<'a> {
    loaded: &'a Loaded,
    input: &'a ProgramInput,
    items: Vec<HashMap<String, Item>>,
    imports: Vec<HashMap<String, usize>>,
    records: Vec<RecordDef>,
    record_decls: Vec<&'a ast::TypeDecl>,
    consts: Vec<ConstInfo<'a>>,
    sigs: Vec<Sig<'a>>,
    errors: Vec<Diagnostic>,
}

struct Fcx {
    module: usize,
    locals: Vec<LocalInfo>,
    scopes: Vec<HashMap<String, LocalId>>,
    ret: Ty,
    loops: usize,
    is_test: bool,
}

impl Fcx {
    fn lookup(&self, name: &str) -> Option<LocalId> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }
}

fn param_pass(mode: ParamMode, ty: &Ty, records: &[RecordDef]) -> PassMode {
    match mode {
        ParamMode::Default if ty.is_copy(records) => PassMode::Copy,
        ParamMode::Default | ParamMode::View => PassMode::Borrow { mutable: false },
        ParamMode::Edit => PassMode::Borrow { mutable: true },
        ParamMode::Take => PassMode::Move,
    }
}

impl<'a> Tc<'a> {
    fn err(&mut self, d: Diagnostic) {
        self.errors.push(d);
    }

    fn ty_name(&self, t: &Ty) -> String {
        t.display(&self.records)
    }

    fn package_name(&self, m: usize) -> &str {
        &self.input.packages[self.loaded.modules[m].package].name
    }

    // ----- phase 1: collect items --------------------------------------

    fn collect(&mut self) {
        for (m, module) in self.loaded.modules.iter().enumerate() {
            for imp in &module.imports {
                self.imports[m].insert(imp.alias.clone(), imp.target);
            }
            let prefix = module_mangle(self.package_name(m), &module.key);
            let is_test_module = matches!(module.key, ModKey::Test(_));
            let mut test_names: HashMap<String, Span> = HashMap::new();
            let mut test_index = 0usize;
            for item in &module.ast.items {
                let (name, span, value) = match item {
                    ast::Item::Use(_) => continue,
                    ast::Item::Fn(f) => {
                        let id = self.sigs.len();
                        self.sigs.push(Sig {
                            name: f.name.name.clone(),
                            module: m,
                            public: f.public,
                            params: Vec::new(),
                            ret: Ty::Void,
                            span: f.name.span,
                            decl: Some(f),
                            test: None,
                            mangled: format!("{prefix}_{}", mangle_len(&f.name.name)),
                        });
                        (&f.name, f.name.span, Item::Fn(id))
                    }
                    ast::Item::Type(t) => {
                        let id = self.records.len();
                        self.records.push(RecordDef {
                            name: t.name.name.clone(),
                            module: m,
                            public: t.public,
                            fields: Vec::new(),
                            span: t.name.span,
                            mangled: format!("{prefix}_T{}", mangle_len(&t.name.name)),
                            is_copy: false,
                            supports_eq: false,
                        });
                        self.record_decls.push(t);
                        (&t.name, t.name.span, Item::Record(id))
                    }
                    ast::Item::Const(c) => {
                        let id = self.consts.len();
                        self.consts.push(ConstInfo {
                            module: m,
                            public: c.public,
                            decl: c,
                            ty: Ty::Error,
                            value: None,
                            state: 0,
                        });
                        (&c.name, c.name.span, Item::Const(id))
                    }
                    ast::Item::Test(t) => {
                        if !is_test_module {
                            self.err(
                                Diagnostic::error(
                                    "B2024",
                                    "`test` blocks are only allowed in files under `tests/`",
                                    t.span,
                                )
                                .with_help("move this test into a file such as `tests/smoke.bv`"),
                            );
                            continue;
                        }
                        if let Some(prev) = test_names.get(&t.name) {
                            let (_, line, _) = self.loaded.sources.location(*prev);
                            self.err(
                                Diagnostic::error(
                                    "B2002",
                                    format!("duplicate test name \"{}\"", t.name),
                                    t.name_span,
                                )
                                .with_note(format!("first defined on line {line}")),
                            );
                            continue;
                        }
                        test_names.insert(t.name.clone(), t.name_span);
                        self.sigs.push(Sig {
                            name: t.name.clone(),
                            module: m,
                            public: false,
                            params: Vec::new(),
                            ret: Ty::Void,
                            span: t.name_span,
                            decl: None,
                            test: Some(t),
                            mangled: format!("{prefix}_test{test_index}"),
                        });
                        test_index += 1;
                        continue;
                    }
                };
                let name_str = name.name.clone();
                if BUILTIN_FNS.contains(&name_str.as_str())
                    || BUILTIN_TYPES.contains(&name_str.as_str())
                {
                    self.err(Diagnostic::error(
                        "B2028",
                        format!("`{name_str}` is a builtin name and cannot be redefined"),
                        span,
                    ));
                    continue;
                }
                if self.imports[m].contains_key(&name_str) {
                    self.err(Diagnostic::error(
                        "B2027",
                        format!("`{name_str}` conflicts with an imported module name"),
                        span,
                    ));
                    continue;
                }
                if let Some(prev) = self.items[m].get(&name_str) {
                    let prev_span = self.item_span(*prev);
                    let (_, line, _) = self.loaded.sources.location(prev_span);
                    self.err(
                        Diagnostic::error(
                            "B2002",
                            format!("`{name_str}` is defined more than once"),
                            span,
                        )
                        .with_note(format!("first defined on line {line}")),
                    );
                    continue;
                }
                self.items[m].insert(name_str, value);
            }
        }
    }

    fn item_span(&self, it: Item) -> Span {
        match it {
            Item::Fn(f) => self.sigs[f].span,
            Item::Record(r) => self.records[r].span,
            Item::Const(c) => self.consts[c].decl.name.span,
        }
    }

    // ----- types -------------------------------------------------------

    fn lookup_qualified(&mut self, m: usize, alias: &str, name: &ast::Ident) -> Option<Item> {
        let target = *self.imports[m].get(alias)?;
        match self.items[target].get(&name.name).copied() {
            None => {
                self.err(Diagnostic::error(
                    "B2001",
                    format!(
                        "module `{}` has no item named `{}`",
                        self.loaded.modules[target].display_name, name.name
                    ),
                    name.span,
                ));
                None
            }
            Some(it) => {
                let public = match it {
                    Item::Fn(f) => self.sigs[f].public,
                    Item::Record(r) => self.records[r].public,
                    Item::Const(c) => self.consts[c].public,
                };
                if !public {
                    self.err(
                        Diagnostic::error(
                            "B2013",
                            format!(
                                "`{}` is private to module `{}`",
                                name.name, self.loaded.modules[target].display_name
                            ),
                            name.span,
                        )
                        .with_help("only declarations marked `pub` can be used from other modules"),
                    );
                    None
                } else {
                    Some(it)
                }
            }
        }
    }

    fn resolve_type(&mut self, m: usize, t: &ast::TypeExpr) -> Ty {
        let base = if t.path.len() == 1 {
            let n = t.path[0].name.as_str();
            match n {
                "Int" => Some(Ty::Int),
                "Float" => Some(Ty::Float),
                "Bool" => Some(Ty::Bool),
                "Byte" => Some(Ty::Byte),
                "String" => Some(Ty::String),
                "Void" => Some(Ty::Void),
                "Buffer" => Some(Ty::Buffer),
                "List" | "Result" => {
                    let Some(arg) = &t.arg else {
                        self.err(Diagnostic::error(
                            "B2009",
                            format!("`{n}` needs an element type, e.g. `{n}[Int]`"),
                            t.span,
                        ));
                        return Ty::Error;
                    };
                    let inner = self.resolve_type(m, arg);
                    if inner == Ty::Void && n == "List" {
                        self.err(Diagnostic::error(
                            "B2019",
                            "`List[Void]` is not a valid type",
                            t.span,
                        ));
                        return Ty::Error;
                    }
                    return if n == "List" {
                        Ty::List(Box::new(inner))
                    } else {
                        Ty::Result(Box::new(inner))
                    };
                }
                _ => match self.items[m].get(n) {
                    Some(Item::Record(r)) => Some(Ty::Record(RecordId(*r))),
                    Some(_) => {
                        self.err(Diagnostic::error(
                            "B2009",
                            format!("`{n}` is not a type"),
                            t.span,
                        ));
                        return Ty::Error;
                    }
                    None => None,
                },
            }
        } else if t.path.len() == 2 && self.imports[m].contains_key(&t.path[0].name) {
            match self.lookup_qualified(m, &t.path[0].name, &t.path[1]) {
                Some(Item::Record(r)) => Some(Ty::Record(RecordId(r))),
                Some(_) => {
                    self.err(Diagnostic::error(
                        "B2009",
                        format!("`{}` is not a type", t.path[1].name),
                        t.span,
                    ));
                    return Ty::Error;
                }
                None => return Ty::Error,
            }
        } else {
            None
        };
        match base {
            Some(ty) => {
                if t.arg.is_some() {
                    self.err(Diagnostic::error(
                        "B2009",
                        format!("`{}` does not take a type argument", self.ty_name(&ty)),
                        t.span,
                    ));
                    return Ty::Error;
                }
                ty
            }
            None => {
                let name = t
                    .path
                    .iter()
                    .map(|i| i.name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                self.err(Diagnostic::error(
                    "B2009",
                    format!("unknown type `{name}`"),
                    t.span,
                ));
                Ty::Error
            }
        }
    }

    // ----- phase 2: records --------------------------------------------

    fn resolve_records(&mut self) {
        for r in 0..self.records.len() {
            let decl = self.record_decls[r];
            let m = self.records[r].module;
            let mut fields: Vec<(String, Ty)> = Vec::new();
            for f in &decl.fields {
                let ty = self.resolve_type(m, &f.ty);
                if ty == Ty::Void {
                    self.err(Diagnostic::error(
                        "B2019",
                        "record fields cannot have type `Void`",
                        f.ty.span,
                    ));
                }
                if fields.iter().any(|(n, _)| *n == f.name.name) {
                    self.err(Diagnostic::error(
                        "B2002",
                        format!("field `{}` is declared more than once", f.name.name),
                        f.name.span,
                    ));
                    continue;
                }
                fields.push((f.name.name.clone(), ty));
            }
            if fields.is_empty() {
                self.err(Diagnostic::error(
                    "B2011",
                    "a record needs at least one field",
                    decl.name.span,
                ));
            }
            self.records[r].fields = fields;
        }
        // By-value recursion (List breaks the cycle; Result does not).
        let n = self.records.len();
        let mut recursive = vec![false; n];
        for (r, rec) in recursive.iter_mut().enumerate() {
            let mut visiting = vec![false; n];
            if self.contains_by_value(r, r, &mut visiting) {
                *rec = true;
                let name = self.records[r].name.clone();
                self.err(
                    Diagnostic::error(
                        "B2021",
                        format!(
                            "record `{name}` contains itself by value and would have infinite size"
                        ),
                        self.records[r].span,
                    )
                    .with_help("store recursive data in a `List[...]` field"),
                );
            }
        }
        // Copy / equality, computed structurally (no by-value cycles remain).
        let mut memo: Vec<Option<(bool, bool)>> = vec![None; n];
        for (r, &is_recursive) in recursive.iter().enumerate() {
            let (c, e) = if is_recursive {
                (false, false)
            } else {
                self.copy_eq(r, &mut memo)
            };
            self.records[r].is_copy = c;
            self.records[r].supports_eq = e;
        }
        // Public records must not expose private records in fields.
        for r in 0..n {
            if self.records[r].public {
                for (fname, fty) in self.records[r].fields.clone() {
                    if let Some(p) = self.private_record_in(&fty, self.records[r].module) {
                        let span = self.record_decls[r]
                            .fields
                            .iter()
                            .find(|f| f.name.name == fname)
                            .map(|f| f.ty.span)
                            .unwrap_or(self.records[r].span);
                        self.err(Diagnostic::error(
                            "B2026",
                            format!("public record field `{fname}` exposes private type `{p}`"),
                            span,
                        ));
                    }
                }
            }
        }
    }

    fn contains_by_value(&self, target: usize, r: usize, visiting: &mut [bool]) -> bool {
        if visiting[r] {
            return false;
        }
        visiting[r] = true;
        for (_, ty) in &self.records[r].fields {
            let mut t = ty;
            while let Ty::Result(inner) = t {
                t = inner;
            }
            if let Ty::Record(RecordId(o)) = t {
                if *o == target || self.contains_by_value(target, *o, visiting) {
                    return true;
                }
            }
        }
        false
    }

    fn copy_eq(&self, r: usize, memo: &mut Vec<Option<(bool, bool)>>) -> (bool, bool) {
        if let Some(v) = memo[r] {
            return v;
        }
        memo[r] = Some((false, false)); // guard
        let mut copy = true;
        let mut eq = true;
        for (_, ty) in &self.records[r].fields {
            match ty {
                Ty::Int | Ty::Float | Ty::Bool | Ty::Byte | Ty::Error => {}
                Ty::String => copy = false,
                Ty::Record(RecordId(o)) => {
                    let (c, e) = self.copy_eq(*o, memo);
                    copy &= c;
                    eq &= e;
                }
                _ => {
                    copy = false;
                    eq = false;
                }
            }
        }
        memo[r] = Some((copy, eq));
        (copy, eq)
    }

    fn private_record_in(&self, ty: &Ty, module: usize) -> Option<String> {
        match ty {
            Ty::Record(RecordId(r))
                if !self.records[*r].public && self.records[*r].module == module =>
            {
                Some(self.records[*r].name.clone())
            }
            Ty::List(t) | Ty::Result(t) => self.private_record_in(t, module),
            _ => None,
        }
    }

    // ----- phase 3: constants ------------------------------------------

    fn eval_consts(&mut self) {
        for c in 0..self.consts.len() {
            self.eval_const_item(c);
        }
    }

    fn eval_const_item(&mut self, c: usize) -> Option<ConstVal> {
        match self.consts[c].state {
            2 => return self.consts[c].value.clone(),
            1 => {
                let d = self.consts[c].decl;
                self.err(Diagnostic::error(
                    "B2023",
                    format!("constant `{}` depends on itself", d.name.name),
                    d.name.span,
                ));
                return None;
            }
            _ => {}
        }
        self.consts[c].state = 1;
        let decl = self.consts[c].decl;
        let m = self.consts[c].module;
        let ty = self.resolve_type(m, &decl.ty);
        if !matches!(ty, Ty::Int | Ty::Float | Ty::Bool | Ty::String | Ty::Error) {
            let name = self.ty_name(&ty);
            self.err(Diagnostic::error(
                "B2023",
                format!("constants must have type Int, Float, Bool or String, not {name}"),
                decl.ty.span,
            ));
        }
        let value = self.eval_const_expr(m, &decl.value);
        if let Some(v) = &value {
            let vty = const_ty(v);
            if !ty.compatible(&vty) {
                let (a, b) = (self.ty_name(&ty), self.ty_name(&vty));
                self.err(Diagnostic::error(
                    "B2003",
                    format!("constant declared as {a} but its value is {b}"),
                    decl.value.span,
                ));
            }
        }
        self.consts[c].ty = ty;
        self.consts[c].value = value.clone();
        self.consts[c].state = 2;
        value
    }

    fn eval_const_expr(&mut self, m: usize, e: &ast::Expr) -> Option<ConstVal> {
        let bad = |this: &mut Self, msg: &str| {
            this.err(Diagnostic::error("B2023", msg.to_string(), e.span));
            None
        };
        match &e.kind {
            AK::Int(v) => Some(ConstVal::Int(*v)),
            AK::Float(v) => Some(ConstVal::Float(*v)),
            AK::Bool(v) => Some(ConstVal::Bool(*v)),
            AK::Str(s) => Some(ConstVal::Str(s.clone())),
            AK::Name(n) => match self.items[m].get(n) {
                Some(Item::Const(c)) => {
                    let c = *c;
                    self.eval_const_item(c)
                }
                _ => bad(
                    self,
                    "constant initializers may only refer to other constants",
                ),
            },
            AK::Field(base, name) => match &base.kind {
                AK::Name(alias) if self.imports[m].contains_key(alias) => {
                    match self.lookup_qualified(m, alias, name) {
                        Some(Item::Const(c)) => self.eval_const_item(c),
                        Some(_) => bad(
                            self,
                            "constant initializers may only refer to other constants",
                        ),
                        None => None,
                    }
                }
                _ => bad(self, "not a constant expression"),
            },
            AK::Unary(op, x) => {
                let v = self.eval_const_expr(m, x)?;
                match (op, v) {
                    (UnOp::Neg, ConstVal::Int(i)) => match i.checked_neg() {
                        Some(r) => Some(ConstVal::Int(r)),
                        None => bad(self, "integer overflow in constant expression"),
                    },
                    (UnOp::Neg, ConstVal::Float(f)) => Some(ConstVal::Float(-f)),
                    (UnOp::Not, ConstVal::Bool(b)) => Some(ConstVal::Bool(!b)),
                    _ => bad(self, "invalid operand type in constant expression"),
                }
            }
            AK::Binary(op, l, r) => {
                let a = self.eval_const_expr(m, l)?;
                let b = self.eval_const_expr(m, r)?;
                use ConstVal::*;
                let res = match (op, &a, &b) {
                    (BinOp::Add, Int(x), Int(y)) => x.checked_add(*y).map(Int),
                    (BinOp::Sub, Int(x), Int(y)) => x.checked_sub(*y).map(Int),
                    (BinOp::Mul, Int(x), Int(y)) => x.checked_mul(*y).map(Int),
                    (BinOp::Div, Int(x), Int(y)) => {
                        if *y == 0 {
                            return bad(self, "division by zero in constant expression");
                        }
                        x.checked_div(*y).map(Int)
                    }
                    (BinOp::Rem, Int(x), Int(y)) => {
                        if *y == 0 {
                            return bad(self, "division by zero in constant expression");
                        }
                        Some(Int(x.checked_rem(*y).unwrap_or(0)))
                    }
                    (BinOp::Add, Float(x), Float(y)) => Some(Float(x + y)),
                    (BinOp::Sub, Float(x), Float(y)) => Some(Float(x - y)),
                    (BinOp::Mul, Float(x), Float(y)) => Some(Float(x * y)),
                    (BinOp::Div, Float(x), Float(y)) => Some(Float(x / y)),
                    (BinOp::And, Bool(x), Bool(y)) => Some(Bool(*x && *y)),
                    (BinOp::Or, Bool(x), Bool(y)) => Some(Bool(*x || *y)),
                    (BinOp::Eq, Int(x), Int(y)) => Some(Bool(x == y)),
                    (BinOp::Ne, Int(x), Int(y)) => Some(Bool(x != y)),
                    (BinOp::Lt, Int(x), Int(y)) => Some(Bool(x < y)),
                    (BinOp::Le, Int(x), Int(y)) => Some(Bool(x <= y)),
                    (BinOp::Gt, Int(x), Int(y)) => Some(Bool(x > y)),
                    (BinOp::Ge, Int(x), Int(y)) => Some(Bool(x >= y)),
                    (BinOp::Eq, Bool(x), Bool(y)) => Some(Bool(x == y)),
                    (BinOp::Ne, Bool(x), Bool(y)) => Some(Bool(x != y)),
                    _ => return bad(self, "unsupported operation in constant expression"),
                };
                match res {
                    Some(v) => Some(v),
                    None => bad(self, "integer overflow in constant expression"),
                }
            }
            _ => bad(
                self,
                "constant values must be literals or arithmetic/Boolean expressions over constants",
            ),
        }
    }

    // ----- phase 4: signatures -----------------------------------------

    fn resolve_sigs(&mut self) {
        for s in 0..self.sigs.len() {
            let Some(decl) = self.sigs[s].decl else {
                continue;
            };
            let m = self.sigs[s].module;
            let mut params = Vec::new();
            for p in &decl.params {
                let ty = self.resolve_type(m, &p.ty);
                if ty == Ty::Void {
                    self.err(Diagnostic::error(
                        "B2019",
                        "parameters cannot have type `Void`",
                        p.ty.span,
                    ));
                }
                if params.iter().any(|q: &SigParam| q.name.name == p.name.name) {
                    self.err(Diagnostic::error(
                        "B2002",
                        format!("parameter `{}` is declared more than once", p.name.name),
                        p.name.span,
                    ));
                }
                params.push(SigParam {
                    name: p.name.clone(),
                    mode: p.mode,
                    ty,
                });
            }
            let ret = match &decl.ret {
                Some(t) => self.resolve_type(m, t),
                None => Ty::Void,
            };
            if decl.public {
                let mut leaks = Vec::new();
                for p in &params {
                    if let Some(n) = self.private_record_in(&p.ty, m) {
                        leaks.push((n, p.name.span));
                    }
                }
                if let Some(n) = self.private_record_in(&ret, m) {
                    leaks.push((n, decl.name.span));
                }
                for (n, span) in leaks {
                    self.err(
                        Diagnostic::error(
                            "B2026",
                            format!(
                                "public function `{}` exposes private type `{n}`",
                                decl.name.name
                            ),
                            span,
                        )
                        .with_help(format!("mark `type {n}` as `pub`")),
                    );
                }
            }
            self.sigs[s].params = params;
            self.sigs[s].ret = ret;
        }
    }

    // ----- phase 5: bodies ---------------------------------------------

    fn check_bodies(&mut self) -> Vec<hir::Function> {
        let mut out = Vec::new();
        for s in 0..self.sigs.len() {
            out.push(self.check_fn(s));
        }
        out
    }

    fn check_fn(&mut self, s: usize) -> hir::Function {
        let m = self.sigs[s].module;
        let mut cx = Fcx {
            module: m,
            locals: Vec::new(),
            scopes: vec![HashMap::new()],
            ret: self.sigs[s].ret.clone(),
            loops: 0,
            is_test: self.sigs[s].test.is_some(),
        };
        let mut params = Vec::new();
        let sig_params: Vec<(ast::Ident, ParamMode, Ty)> = self.sigs[s]
            .params
            .iter()
            .map(|p| (p.name.clone(), p.mode, p.ty.clone()))
            .collect();
        for (name, mode, ty) in sig_params {
            let pass = param_pass(mode, &ty, &self.records);
            let kind = match pass {
                PassMode::Copy | PassMode::Move => LocalKind::Owned { mutable: false },
                PassMode::Borrow { mutable } => LocalKind::Ref { mutable },
            };
            let local = self.declare(&mut cx, &name, ty, kind, true);
            params.push(hir::Param { local, pass });
        }
        let body_ast = match (self.sigs[s].decl, self.sigs[s].test) {
            (Some(d), _) => &d.body,
            (_, Some(t)) => &t.body,
            _ => unreachable!(),
        };
        let body = self.block_in_current_scope(&mut cx, body_ast);
        let ret = self.sigs[s].ret.clone();
        if ret != Ty::Void && !ret.is_error() && !block_returns(&body) {
            let name = self.sigs[s].name.clone();
            let span = self.sigs[s].span;
            let t = self.ty_name(&ret);
            self.err(
                Diagnostic::error(
                    "B2006",
                    format!(
                        "function `{name}` may reach the end of its body without returning a {t}"
                    ),
                    span,
                )
                .with_help("add a `return` on every path (an `else:` branch is often missing)"),
            );
        }
        hir::Function {
            name: self.sigs[s].name.clone(),
            module: m,
            public: self.sigs[s].public,
            params,
            ret,
            locals: cx.locals,
            body,
            span: self.sigs[s].span,
            mangled: self.sigs[s].mangled.clone(),
            test_name: self.sigs[s].test.map(|t| t.name.clone()),
        }
    }

    fn declare(
        &mut self,
        cx: &mut Fcx,
        name: &ast::Ident,
        ty: Ty,
        kind: LocalKind,
        is_param: bool,
    ) -> LocalId {
        if self.imports[cx.module].contains_key(&name.name) {
            self.err(Diagnostic::error(
                "B2027",
                format!("`{}` conflicts with an imported module name", name.name),
                name.span,
            ));
        }
        if let Some(prev) = cx.scopes.last().unwrap().get(&name.name) {
            let (_, line, _) = self.loaded.sources.location(cx.locals[prev.0].span);
            self.err(
                Diagnostic::error(
                    "B2002",
                    format!("`{}` is already declared in this scope", name.name),
                    name.span,
                )
                .with_note(format!("previous declaration on line {line}"))
                .with_help("use a different name, assign with `=` to a `var`, or shadow it in a nested block"),
            );
        }
        let id = LocalId(cx.locals.len());
        cx.locals.push(LocalInfo {
            name: name.name.clone(),
            ty,
            kind,
            span: name.span,
            is_param,
        });
        cx.scopes.last_mut().unwrap().insert(name.name.clone(), id);
        id
    }

    fn block(&mut self, cx: &mut Fcx, b: &ast::Block) -> hir::Block {
        cx.scopes.push(HashMap::new());
        let out = self.block_in_current_scope(cx, b);
        cx.scopes.pop();
        out
    }

    fn block_in_current_scope(&mut self, cx: &mut Fcx, b: &ast::Block) -> hir::Block {
        let mut stmts = Vec::new();
        for s in &b.stmts {
            if let Some(st) = self.stmt(cx, s) {
                stmts.push(st);
            }
        }
        hir::Block {
            stmts,
            span: b.span,
        }
    }

    fn stmt(&mut self, cx: &mut Fcx, s: &ast::Stmt) -> Option<hir::Stmt> {
        use hir::StmtKind as SK;
        let kind = match &s.kind {
            ast::StmtKind::Let {
                mutable,
                name,
                ty,
                init,
            } => {
                let annotated = ty.as_ref().map(|t| self.resolve_type(cx.module, t));
                if let AK::Borrow(kind, place) = &init.kind {
                    let place_e = self.place(cx, place, "borrow");
                    if *mutable {
                        self.err(
                            Diagnostic::error(
                                "B3013",
                                "a reference cannot be bound with `var`",
                                s.span,
                            )
                            .with_note("references cannot be re-pointed; use `let` (writes through an `edit` reference are still allowed)"),
                        );
                    }
                    if let Some(a) = &annotated {
                        self.expect_ty(a, &place_e.ty, place.span);
                    }
                    let ty = place_e.ty.clone();
                    let init_e = hir::Expr {
                        ty: ty.clone(),
                        span: init.span,
                        kind: hir::ExprKind::Borrow(*kind, Box::new(place_e)),
                    };
                    let local = self.declare(
                        cx,
                        name,
                        ty,
                        LocalKind::Ref {
                            mutable: *kind == BorrowKind::Edit,
                        },
                        false,
                    );
                    SK::Let {
                        local,
                        init: init_e,
                    }
                } else {
                    let init_e = self.expr(cx, init, annotated.as_ref());
                    let ty = match annotated {
                        Some(a) => {
                            self.expect_ty(&a, &init_e.ty, init.span);
                            a
                        }
                        None => init_e.ty.clone(),
                    };
                    if ty == Ty::Void {
                        self.err(Diagnostic::error(
                            "B2019",
                            "cannot bind a `Void` value to a variable",
                            init.span,
                        ));
                    }
                    let local =
                        self.declare(cx, name, ty, LocalKind::Owned { mutable: *mutable }, false);
                    SK::Let {
                        local,
                        init: init_e,
                    }
                }
            }
            ast::StmtKind::Assign { target, op, value } => {
                let target_e = self.place(cx, target, "assign to");
                if let Some(root) = target_e.place_root() {
                    let info = cx.locals[root.0].clone();
                    match info.kind {
                        LocalKind::Owned { mutable: false } => {
                            let what = if info.is_param {
                                "parameter"
                            } else {
                                "immutable variable"
                            };
                            let mut d = Diagnostic::error(
                                "B2007",
                                format!("cannot assign to {what} `{}`", info.name),
                                target.span,
                            );
                            d = if info.is_param {
                                d.with_help(format!(
                                    "copy it into a local first (`var {0}_copy = {0}`) or declare the parameter as `edit`",
                                    info.name
                                ))
                            } else {
                                d.with_help(format!(
                                    "declare it with `var {}` to make it assignable",
                                    info.name
                                ))
                            };
                            self.err(d);
                        }
                        LocalKind::Ref { mutable: false } => {
                            self.err(Diagnostic::error(
                                "B3011",
                                format!("cannot assign through read-only reference `{}`", info.name),
                                target.span,
                            )
                            .with_help("a `view` reference (or plain non-Copy parameter) is read-only; use `edit`"));
                        }
                        _ => {}
                    }
                }
                let value_e = self.expr(cx, value, Some(&target_e.ty));
                self.expect_ty(&target_e.ty, &value_e.ty, value.span);
                if *op != AssignOp::Set && !matches!(target_e.ty, Ty::Int | Ty::Float | Ty::Error) {
                    let t = self.ty_name(&target_e.ty);
                    self.err(Diagnostic::error(
                        "B2005",
                        format!("compound assignment needs an Int or Float target, found {t}"),
                        target.span,
                    ));
                }
                SK::Assign {
                    target: target_e,
                    op: *op,
                    value: value_e,
                }
            }
            ast::StmtKind::Return(value) => {
                let ret = cx.ret.clone();
                match value {
                    None => {
                        if ret != Ty::Void && !ret.is_error() {
                            let t = self.ty_name(&ret);
                            self.err(Diagnostic::error(
                                "B2029",
                                format!("this function must return a value of type {t}"),
                                s.span,
                            ));
                        }
                        SK::Return(None)
                    }
                    Some(v) => {
                        if ret == Ty::Void {
                            self.err(
                                Diagnostic::error(
                                    "B2029",
                                    "a function without a return type cannot return a value",
                                    v.span,
                                )
                                .with_help("add `-> Type` to the function signature"),
                            );
                        }
                        let e = self.expr(cx, v, Some(&ret));
                        if ret != Ty::Void {
                            self.expect_ty(&ret, &e.ty, v.span);
                        }
                        SK::Return(Some(e))
                    }
                }
            }
            ast::StmtKind::If { cond, then, els } => {
                let cond = self.condition(cx, cond, "if");
                let then = self.block(cx, then);
                let els = els.as_ref().map(|b| self.block(cx, b));
                SK::If { cond, then, els }
            }
            ast::StmtKind::While { cond, body } => {
                let cond = self.condition(cx, cond, "while");
                cx.loops += 1;
                let body = self.block(cx, body);
                cx.loops -= 1;
                SK::While { cond, body }
            }
            ast::StmtKind::For { var, iter, body } => {
                let is_range = matches!(&iter.kind, AK::Call(c, _) if matches!(&c.kind, AK::Name(n) if n == "range" && cx.lookup("range").is_none()));
                cx.scopes.push(HashMap::new());
                let kind = if is_range {
                    let AK::Call(_, args) = &iter.kind else {
                        unreachable!()
                    };
                    if args.len() != 2 || args.iter().any(|a| a.name.is_some()) {
                        self.err(Diagnostic::error(
                            "B2010",
                            "`range` takes exactly two arguments: range(start, end)",
                            iter.span,
                        ));
                    }
                    let mut bounds = Vec::new();
                    for a in args.iter().take(2) {
                        let e = self.expr(cx, &a.value, Some(&Ty::Int));
                        self.expect_ty(&Ty::Int, &e.ty, a.value.span);
                        bounds.push(e);
                    }
                    while bounds.len() < 2 {
                        bounds.push(hir::Expr {
                            kind: hir::ExprKind::Int(0),
                            ty: Ty::Int,
                            span: iter.span,
                        });
                    }
                    let end = bounds.pop().unwrap();
                    let start = bounds.pop().unwrap();
                    let v =
                        self.declare(cx, var, Ty::Int, LocalKind::Owned { mutable: false }, false);
                    cx.loops += 1;
                    let body = self.block(cx, body);
                    cx.loops -= 1;
                    SK::ForRange {
                        var: v,
                        start,
                        end,
                        body,
                    }
                } else {
                    let iter_e = self.expr(cx, iter, None);
                    let elem = match &iter_e.ty {
                        Ty::List(t) => (**t).clone(),
                        Ty::Error => Ty::Error,
                        other => {
                            let t = self.ty_name(other);
                            self.err(Diagnostic::error(
                                "B2005",
                                format!("cannot iterate over {t}; `for` supports List values and range(start, end)"),
                                iter.span,
                            ));
                            Ty::Error
                        }
                    };
                    let kind = if elem.is_copy(&self.records) {
                        LocalKind::Owned { mutable: false }
                    } else {
                        LocalKind::Ref { mutable: false }
                    };
                    let v = self.declare(cx, var, elem, kind, false);
                    cx.loops += 1;
                    let body = self.block(cx, body);
                    cx.loops -= 1;
                    SK::ForList {
                        var: v,
                        iter: iter_e,
                        body,
                    }
                };
                cx.scopes.pop();
                kind
            }
            ast::StmtKind::Break | ast::StmtKind::Continue => {
                let is_break = matches!(s.kind, ast::StmtKind::Break);
                if cx.loops == 0 {
                    self.err(Diagnostic::error(
                        "B2022",
                        format!(
                            "`{}` outside of a loop",
                            if is_break { "break" } else { "continue" }
                        ),
                        s.span,
                    ));
                }
                if is_break {
                    SK::Break
                } else {
                    SK::Continue
                }
            }
            ast::StmtKind::Scope(b) => SK::Scope(self.block(cx, b)),
            ast::StmtKind::Pass => return None,
            ast::StmtKind::Expr(e) => SK::Expr(self.expr(cx, e, None)),
        };
        Some(hir::Stmt { kind, span: s.span })
    }

    fn condition(&mut self, cx: &mut Fcx, e: &ast::Expr, what: &str) -> hir::Expr {
        let c = self.expr(cx, e, Some(&Ty::Bool));
        if !c.ty.compatible(&Ty::Bool) {
            let t = self.ty_name(&c.ty);
            self.err(
                Diagnostic::error(
                    "B2004",
                    format!("`{what}` condition must be Bool, found {t}"),
                    e.span,
                )
                .with_help("Beaver has no truthiness; compare explicitly, e.g. `x != 0`"),
            );
        }
        c
    }

    fn expect_ty(&mut self, expected: &Ty, found: &Ty, span: Span) {
        if !expected.compatible(found) {
            let (a, b) = (self.ty_name(expected), self.ty_name(found));
            let mut d = Diagnostic::error(
                "B2003",
                format!("type mismatch: expected {a}, found {b}"),
                span,
            );
            if matches!(
                (expected, found),
                (Ty::Float, Ty::Int)
                    | (Ty::Int, Ty::Float)
                    | (Ty::Byte, Ty::Int)
                    | (Ty::Int, Ty::Byte)
            ) {
                d = d.with_help(format!(
                    "there are no implicit numeric conversions; use `{a}(...)`"
                ));
            }
            self.err(d);
        }
    }

    // ----- expressions -------------------------------------------------

    fn mk(kind: hir::ExprKind, ty: Ty, span: Span) -> hir::Expr {
        hir::Expr { kind, ty, span }
    }

    fn err_expr(span: Span) -> hir::Expr {
        Self::mk(hir::ExprKind::Int(0), Ty::Error, span)
    }

    fn const_expr(&self, c: usize, span: Span) -> hir::Expr {
        match &self.consts[c].value {
            Some(ConstVal::Int(v)) => Self::mk(hir::ExprKind::Int(*v), Ty::Int, span),
            Some(ConstVal::Float(v)) => Self::mk(hir::ExprKind::Float(*v), Ty::Float, span),
            Some(ConstVal::Bool(v)) => Self::mk(hir::ExprKind::Bool(*v), Ty::Bool, span),
            Some(ConstVal::Str(v)) => Self::mk(hir::ExprKind::Str(v.clone()), Ty::String, span),
            None => Self::err_expr(span),
        }
    }

    /// Type-check an expression that must denote storage (assignment targets,
    /// borrow and take operands).
    fn place(&mut self, cx: &mut Fcx, e: &ast::Expr, what: &str) -> hir::Expr {
        match &e.kind {
            AK::Name(n) => match cx.lookup(n) {
                Some(l) => Self::mk(hir::ExprKind::Local(l), cx.locals[l.0].ty.clone(), e.span),
                None => {
                    let msg = if self.items[cx.module].contains_key(n) {
                        format!("cannot {what} `{n}`: only local variables and parameters denote storage")
                    } else {
                        format!("unknown name `{n}`")
                    };
                    self.err(Diagnostic::error("B2001", msg, e.span));
                    Self::err_expr(e.span)
                }
            },
            AK::Field(base, f) => {
                if let AK::Name(alias) = &base.kind {
                    if cx.lookup(alias).is_none() && self.imports[cx.module].contains_key(alias) {
                        self.err(Diagnostic::error(
                            "B2001",
                            format!(
                                "cannot {what} `{alias}.{}`: module items are not storage",
                                f.name
                            ),
                            e.span,
                        ));
                        return Self::err_expr(e.span);
                    }
                }
                let b = self.place(cx, base, what);
                self.field_of(b, f, e.span)
            }
            AK::Index(base, idx) => {
                let b = self.place(cx, base, what);
                self.index_of(cx, b, idx, e.span)
            }
            _ => {
                self.err(Diagnostic::error(
                    "B1014",
                    format!("cannot {what} this expression"),
                    e.span,
                ));
                Self::err_expr(e.span)
            }
        }
    }

    fn field_of(&mut self, b: hir::Expr, f: &ast::Ident, span: Span) -> hir::Expr {
        match &b.ty {
            Ty::Record(RecordId(r)) => {
                let r = *r;
                match self.records[r]
                    .fields
                    .iter()
                    .position(|(n, _)| *n == f.name)
                {
                    Some(i) => {
                        let ty = self.records[r].fields[i].1.clone();
                        Self::mk(hir::ExprKind::Field(Box::new(b), i), ty, span)
                    }
                    None => {
                        let rn = self.records[r].name.clone();
                        self.err(Diagnostic::error(
                            "B2011",
                            format!("record `{rn}` has no field `{}`", f.name),
                            f.span,
                        ));
                        Self::err_expr(span)
                    }
                }
            }
            Ty::Error => Self::err_expr(span),
            other => {
                let t = self.ty_name(other);
                self.err(Diagnostic::error(
                    "B2011",
                    format!("type {t} has no fields (no field `{}`)", f.name),
                    f.span,
                ));
                Self::err_expr(span)
            }
        }
    }

    fn index_of(&mut self, cx: &mut Fcx, b: hir::Expr, idx: &ast::Expr, span: Span) -> hir::Expr {
        let i = self.expr(cx, idx, Some(&Ty::Int));
        if !i.ty.compatible(&Ty::Int) {
            let t = self.ty_name(&i.ty);
            self.err(Diagnostic::error(
                "B2016",
                format!("index must be Int, found {t}"),
                idx.span,
            ));
        }
        let ty = match &b.ty {
            Ty::List(t) => (**t).clone(),
            Ty::Buffer => Ty::Byte,
            Ty::Error => Ty::Error,
            Ty::String => {
                self.err(
                    Diagnostic::error("B2017", "Strings cannot be indexed in Beaver 0.1", span)
                        .with_note("String indexing by byte would split UTF-8 characters"),
                );
                Ty::Error
            }
            other => {
                let t = self.ty_name(other);
                self.err(Diagnostic::error(
                    "B2016",
                    format!("type {t} cannot be indexed"),
                    span,
                ));
                Ty::Error
            }
        };
        Self::mk(hir::ExprKind::Index(Box::new(b), Box::new(i)), ty, span)
    }

    fn expr(&mut self, cx: &mut Fcx, e: &ast::Expr, expected: Option<&Ty>) -> hir::Expr {
        use hir::ExprKind as HK;
        let span = e.span;
        match &e.kind {
            AK::Int(v) => Self::mk(HK::Int(*v), Ty::Int, span),
            AK::Float(v) => Self::mk(HK::Float(*v), Ty::Float, span),
            AK::Str(s) => Self::mk(HK::Str(s.clone()), Ty::String, span),
            AK::Bool(b) => Self::mk(HK::Bool(*b), Ty::Bool, span),
            AK::Name(n) => {
                if let Some(l) = cx.lookup(n) {
                    return Self::mk(HK::Local(l), cx.locals[l.0].ty.clone(), span);
                }
                match self.items[cx.module].get(n).copied() {
                    Some(Item::Const(c)) => return self.const_expr(c, span),
                    Some(Item::Fn(_)) => {
                        self.err(Diagnostic::error(
                            "B2012",
                            format!("function `{n}` is not a value; call it with `{n}(...)`"),
                            span,
                        ));
                        return Self::err_expr(span);
                    }
                    Some(Item::Record(_)) => {
                        self.err(Diagnostic::error(
                            "B2012",
                            format!(
                                "type `{n}` is not a value; construct it with `{n}(field=...)`"
                            ),
                            span,
                        ));
                        return Self::err_expr(span);
                    }
                    None => {}
                }
                let msg = if self.imports[cx.module].contains_key(n) {
                    format!("module `{n}` is not a value")
                } else if BUILTIN_FNS.contains(&n.as_str()) {
                    format!("builtin `{n}` must be called")
                } else {
                    format!("unknown name `{n}`")
                };
                self.err(Diagnostic::error("B2001", msg, span));
                Self::err_expr(span)
            }
            AK::List(elems) => {
                let want = match expected {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                if elems.is_empty() {
                    return match want {
                        Some(t) => Self::mk(HK::List(Vec::new()), Ty::List(Box::new(t)), span),
                        None => {
                            self.err(
                                Diagnostic::error(
                                    "B2018",
                                    "cannot infer the element type of `[]`",
                                    span,
                                )
                                .with_help("annotate the variable, e.g. `var xs: List[Int] = []`"),
                            );
                            Self::err_expr(span)
                        }
                    };
                }
                let mut out = Vec::new();
                let mut elem_ty = want.clone();
                for el in elems {
                    let x = self.expr(cx, el, elem_ty.as_ref());
                    match &elem_ty {
                        Some(t) => self.expect_ty(t, &x.ty, el.span),
                        None => elem_ty = Some(x.ty.clone()),
                    }
                    if x.ty == Ty::Void {
                        self.err(Diagnostic::error(
                            "B2019",
                            "list elements cannot be Void",
                            el.span,
                        ));
                    }
                    out.push(x);
                }
                Self::mk(HK::List(out), Ty::List(Box::new(elem_ty.unwrap())), span)
            }
            AK::Binary(op, l, r) => self.binary(cx, *op, l, r, span),
            AK::Unary(op, x) => {
                let x = self.expr(cx, x, None);
                let ok = match op {
                    UnOp::Neg => matches!(x.ty, Ty::Int | Ty::Float | Ty::Error),
                    UnOp::Not => matches!(x.ty, Ty::Bool | Ty::Error),
                };
                if !ok {
                    let t = self.ty_name(&x.ty);
                    let what = if *op == UnOp::Neg { "-" } else { "not" };
                    self.err(Diagnostic::error(
                        "B2005",
                        format!("cannot apply `{what}` to {t}"),
                        span,
                    ));
                    return Self::err_expr(span);
                }
                let ty = x.ty.clone();
                Self::mk(HK::Unary(*op, Box::new(x)), ty, span)
            }
            AK::Borrow(kind, place) => {
                let p = self.place(cx, place, "borrow");
                self.err(
                    Diagnostic::error(
                        "B3016",
                        format!(
                            "`{}` can only appear in a `let` binding or as a function argument",
                            if *kind == BorrowKind::View {
                                "view"
                            } else {
                                "edit"
                            }
                        ),
                        span,
                    )
                    .with_note(
                        "references cannot be stored in lists, records or Results, or returned",
                    ),
                );
                p
            }
            AK::Take(place) => {
                let p = self.place(cx, place, "take");
                let ty = p.ty.clone();
                Self::mk(HK::Take(Box::new(p)), ty, span)
            }
            AK::Try(x) => {
                if !matches!(cx.ret, Ty::Result(_) | Ty::Error) {
                    let msg = if cx.is_test {
                        "`try` cannot be used in a test block".to_string()
                    } else {
                        "`try` can only be used inside a function that returns Result[T]"
                            .to_string()
                    };
                    self.err(
                        Diagnostic::error("B2008", msg, span)
                            .with_help("use `unwrap(...)` or check `is_ok(...)` here, or make this function return a Result"),
                    );
                }
                let x = self.expr(cx, x, None);
                let ty = match &x.ty {
                    Ty::Result(t) => (**t).clone(),
                    Ty::Error => Ty::Error,
                    other => {
                        let t = self.ty_name(other);
                        self.err(Diagnostic::error(
                            "B2008",
                            format!("`try` needs a Result value, found {t}"),
                            span,
                        ));
                        Ty::Error
                    }
                };
                Self::mk(HK::Try(Box::new(x)), ty, span)
            }
            AK::Call(callee, args) => self.call(cx, callee, args, expected, span),
            AK::Field(base, f) => {
                if let AK::Name(alias) = &base.kind {
                    if cx.lookup(alias).is_none() && self.imports[cx.module].contains_key(alias) {
                        return match self.lookup_qualified(cx.module, alias, f) {
                            Some(Item::Const(c)) => self.const_expr(c, span),
                            Some(_) => {
                                self.err(Diagnostic::error(
                                    "B2012",
                                    format!(
                                        "`{alias}.{}` is not a value; call or construct it",
                                        f.name
                                    ),
                                    span,
                                ));
                                Self::err_expr(span)
                            }
                            None => Self::err_expr(span),
                        };
                    }
                }
                let b = self.expr(cx, base, None);
                self.field_of(b, f, span)
            }
            AK::Index(base, idx) => {
                let b = self.expr(cx, base, None);
                self.index_of(cx, b, idx, span)
            }
        }
    }

    fn binary(
        &mut self,
        cx: &mut Fcx,
        op: BinOp,
        l: &ast::Expr,
        r: &ast::Expr,
        span: Span,
    ) -> hir::Expr {
        let a = self.expr(cx, l, None);
        let b = self.expr(cx, r, None);
        let (ta, tb) = (a.ty.clone(), b.ty.clone());
        let any_err = ta.is_error() || tb.is_error();
        let result = match op {
            BinOp::And | BinOp::Or => {
                if !(ta.compatible(&Ty::Bool) && tb.compatible(&Ty::Bool)) {
                    None
                } else {
                    Some(Ty::Bool)
                }
            }
            BinOp::Add => match (&ta, &tb) {
                (Ty::Int, Ty::Int) => Some(Ty::Int),
                (Ty::Float, Ty::Float) => Some(Ty::Float),
                (Ty::String, Ty::String) => Some(Ty::String),
                _ if any_err => Some(if ta.is_error() {
                    tb.clone()
                } else {
                    ta.clone()
                }),
                _ => None,
            },
            BinOp::Sub | BinOp::Mul | BinOp::Div => match (&ta, &tb) {
                (Ty::Int, Ty::Int) => Some(Ty::Int),
                (Ty::Float, Ty::Float) => Some(Ty::Float),
                _ if any_err => Some(Ty::Error),
                _ => None,
            },
            BinOp::Rem => match (&ta, &tb) {
                (Ty::Int, Ty::Int) => Some(Ty::Int),
                _ if any_err => Some(Ty::Error),
                _ => None,
            },
            BinOp::Eq | BinOp::Ne => {
                if !ta.compatible(&tb) {
                    None
                } else if !ta.supports_eq(&self.records) {
                    let t = self.ty_name(&ta);
                    self.err(Diagnostic::error(
                        "B2025",
                        format!(
                            "values of type {t} cannot be compared with `{}`",
                            op.symbol()
                        ),
                        span,
                    ));
                    return Self::err_expr(span);
                } else {
                    Some(Ty::Bool)
                }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (&ta, &tb) {
                (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float) | (Ty::Byte, Ty::Byte) => {
                    Some(Ty::Bool)
                }
                _ if any_err => Some(Ty::Bool),
                _ => None,
            },
        };
        match result {
            Some(ty) => Self::mk(
                hir::ExprKind::Binary(op, Box::new(a), Box::new(b)),
                ty,
                span,
            ),
            None => {
                let (x, y) = (self.ty_name(&ta), self.ty_name(&tb));
                let mut d = Diagnostic::error(
                    "B2005",
                    format!("cannot apply `{}` to {x} and {y}", op.symbol()),
                    span,
                );
                if matches!((&ta, &tb), (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int)) {
                    d = d.with_help("convert explicitly with `Float(...)` or `Int(...)`");
                } else if op == BinOp::Add && (ta == Ty::String || tb == Ty::String) {
                    d = d.with_help("only String + String is supported; there is no implicit conversion to String");
                } else if matches!(op, BinOp::And | BinOp::Or) {
                    d = d.with_help("`and`/`or` need Bool operands");
                }
                self.err(d);
                Self::err_expr(span)
            }
        }
    }

    /// Argument read by a builtin: `x`, `view x`, or any temporary.
    fn read_arg(&mut self, cx: &mut Fcx, a: &ast::Arg) -> hir::Expr {
        match &a.value.kind {
            AK::Borrow(BorrowKind::View, p) => self.place(cx, p, "view"),
            AK::Borrow(BorrowKind::Edit, _) => {
                self.err(Diagnostic::error(
                    "B3017",
                    "this builtin only reads its argument; remove `edit`",
                    a.value.span,
                ));
                Self::err_expr(a.value.span)
            }
            _ => self.expr(cx, &a.value, None),
        }
    }

    fn call(
        &mut self,
        cx: &mut Fcx,
        callee: &ast::Expr,
        args: &[ast::Arg],
        expected: Option<&Ty>,
        span: Span,
    ) -> hir::Expr {
        let item = match &callee.kind {
            AK::Name(n) => {
                if cx.lookup(n).is_some() {
                    self.err(Diagnostic::error(
                        "B2012",
                        format!("`{n}` is a variable, not a function"),
                        callee.span,
                    ));
                    return Self::err_expr(span);
                }
                match self.items[cx.module].get(n).copied() {
                    Some(it) => it,
                    None => return self.builtin_call(cx, n, callee.span, args, expected, span),
                }
            }
            AK::Field(base, f) => match &base.kind {
                AK::Name(alias)
                    if cx.lookup(alias).is_none()
                        && self.imports[cx.module].contains_key(alias) =>
                {
                    match self.lookup_qualified(cx.module, alias, f) {
                        Some(it) => it,
                        None => return Self::err_expr(span),
                    }
                }
                _ => {
                    self.err(
                        Diagnostic::error("B2012", "this expression cannot be called", callee.span)
                            .with_note("Beaver 0.1 has no methods; call functions as `f(x)` or `module.f(x)`"),
                    );
                    return Self::err_expr(span);
                }
            },
            _ => {
                self.err(Diagnostic::error(
                    "B2012",
                    "this expression cannot be called",
                    callee.span,
                ));
                return Self::err_expr(span);
            }
        };
        match item {
            Item::Const(_) => {
                self.err(Diagnostic::error(
                    "B2012",
                    "a constant cannot be called",
                    callee.span,
                ));
                Self::err_expr(span)
            }
            Item::Record(r) => self.construct(cx, r, args, span),
            Item::Fn(f) => self.user_call(cx, f, args, span),
        }
    }

    fn user_call(&mut self, cx: &mut Fcx, f: usize, args: &[ast::Arg], span: Span) -> hir::Expr {
        let name = self.sigs[f].name.clone();
        if let Some(a) = args.iter().find(|a| a.name.is_some()) {
            self.err(Diagnostic::error(
                "B2010",
                format!("function `{name}` takes positional arguments, not named ones"),
                a.name.as_ref().unwrap().span,
            ));
        }
        let nparams = self.sigs[f].params.len();
        if args.len() != nparams {
            self.err(Diagnostic::error(
                "B2010",
                format!(
                    "`{name}` expects {nparams} argument(s) but {} were given",
                    args.len()
                ),
                span,
            ));
            return Self::mk(hir::ExprKind::Int(0), self.sigs[f].ret.clone(), span);
        }
        let mut out = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let (pname, mode, pty) = {
                let p = &self.sigs[f].params[i];
                (p.name.name.clone(), p.mode, p.ty.clone())
            };
            let pass = param_pass(mode, &pty, &self.records);
            let v = &a.value;
            let arg = match (pass, &v.kind) {
                (PassMode::Borrow { mutable: true }, AK::Borrow(BorrowKind::Edit, p)) => {
                    let pe = self.place(cx, p, "edit");
                    let ty = pe.ty.clone();
                    Self::mk(
                        hir::ExprKind::Borrow(BorrowKind::Edit, Box::new(pe)),
                        ty,
                        v.span,
                    )
                }
                (PassMode::Borrow { mutable: true }, _) => {
                    self.err(
                        Diagnostic::error(
                            "B3017",
                            format!(
                                "parameter `{pname}` of `{name}` is `edit`; pass `edit <variable>`"
                            ),
                            v.span,
                        )
                        .with_note("mutation through a call must be visible at the call site"),
                    );
                    Self::err_expr(v.span)
                }
                (PassMode::Borrow { mutable: false }, AK::Borrow(BorrowKind::View, p)) => {
                    let pe = self.place(cx, p, "view");
                    let ty = pe.ty.clone();
                    Self::mk(
                        hir::ExprKind::Borrow(BorrowKind::View, Box::new(pe)),
                        ty,
                        v.span,
                    )
                }
                (PassMode::Borrow { mutable: false }, AK::Borrow(BorrowKind::Edit, _)) => {
                    self.err(Diagnostic::error(
                        "B3017",
                        format!("parameter `{pname}` of `{name}` is read-only; remove `edit`"),
                        v.span,
                    ));
                    Self::err_expr(v.span)
                }
                (PassMode::Borrow { mutable: false }, AK::Take(_)) => {
                    self.err(Diagnostic::error(
                        "B3017",
                        format!("parameter `{pname}` of `{name}` only borrows its argument; remove `take`"),
                        v.span,
                    ));
                    Self::err_expr(v.span)
                }
                (PassMode::Copy | PassMode::Move, AK::Borrow(..)) => {
                    let what = if pass == PassMode::Copy {
                        "is passed by value"
                    } else {
                        "takes ownership (`take`)"
                    };
                    self.err(Diagnostic::error(
                        "B3017",
                        format!("parameter `{pname}` of `{name}` {what}; a reference cannot be passed here"),
                        v.span,
                    ));
                    Self::err_expr(v.span)
                }
                _ => self.expr(cx, v, Some(&pty)),
            };
            self.expect_ty(&pty, &arg.ty, v.span);
            out.push((pass, arg));
        }
        let ret = self.sigs[f].ret.clone();
        Self::mk(hir::ExprKind::Call(FnId(f), out), ret, span)
    }

    fn construct(&mut self, cx: &mut Fcx, r: usize, args: &[ast::Arg], span: Span) -> hir::Expr {
        let rname = self.records[r].name.clone();
        let mut seen: Vec<Option<Span>> = vec![None; self.records[r].fields.len()];
        let mut out = Vec::new();
        for a in args {
            let Some(n) = &a.name else {
                self.err(
                    Diagnostic::error(
                        "B2011",
                        format!("record constructor `{rname}` needs named fields"),
                        a.value.span,
                    )
                    .with_help(format!("write `{rname}(field=value, ...)`")),
                );
                continue;
            };
            let Some(i) = self.records[r]
                .fields
                .iter()
                .position(|(f, _)| *f == n.name)
            else {
                self.err(Diagnostic::error(
                    "B2011",
                    format!("record `{rname}` has no field `{}`", n.name),
                    n.span,
                ));
                continue;
            };
            if seen[i].is_some() {
                self.err(Diagnostic::error(
                    "B2011",
                    format!("field `{}` is given more than once", n.name),
                    n.span,
                ));
                continue;
            }
            seen[i] = Some(n.span);
            let fty = self.records[r].fields[i].1.clone();
            let v = self.expr(cx, &a.value, Some(&fty));
            self.expect_ty(&fty, &v.ty, a.value.span);
            out.push((i, v));
        }
        let missing: Vec<String> = seen
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_none())
            .map(|(i, _)| self.records[r].fields[i].0.clone())
            .collect();
        if !missing.is_empty() {
            self.err(Diagnostic::error(
                "B2011",
                format!("missing field(s) for `{rname}`: {}", missing.join(", ")),
                span,
            ));
        }
        Self::mk(
            hir::ExprKind::Construct(RecordId(r), out),
            Ty::Record(RecordId(r)),
            span,
        )
    }

    fn builtin_call(
        &mut self,
        cx: &mut Fcx,
        name: &str,
        name_span: Span,
        args: &[ast::Arg],
        expected: Option<&Ty>,
        span: Span,
    ) -> hir::Expr {
        use hir::ExprKind as HK;
        let known =
            BUILTIN_FNS.contains(&name) || matches!(name, "Int" | "Float" | "Byte" | "Buffer");
        if !known {
            self.err(Diagnostic::error(
                "B2001",
                format!("unknown function `{name}`"),
                name_span,
            ));
            return Self::err_expr(span);
        }
        if name != "Buffer" {
            if let Some(a) = args.iter().find(|a| a.name.is_some()) {
                self.err(Diagnostic::error(
                    "B2010",
                    format!("`{name}` does not take named arguments"),
                    a.name.as_ref().unwrap().span,
                ));
                return Self::err_expr(span);
            }
        }
        let arity = |this: &mut Self, n: usize| -> bool {
            if args.len() != n {
                this.err(Diagnostic::error(
                    "B2010",
                    format!(
                        "`{name}` expects {n} argument(s) but {} were given",
                        args.len()
                    ),
                    span,
                ));
                false
            } else {
                true
            }
        };
        let bad_ty = |this: &mut Self, what: &str, t: &Ty, s: Span| {
            let tn = this.ty_name(t);
            this.err(Diagnostic::error(
                "B2005",
                format!("`{name}` {what}, found {tn}"),
                s,
            ));
        };
        match name {
            "print" => {
                if !arity(self, 1) {
                    return Self::err_expr(span);
                }
                let x = self.read_arg(cx, &args[0]);
                if x.ty == Ty::Void {
                    self.err(Diagnostic::error(
                        "B2019",
                        "cannot print a Void value",
                        args[0].value.span,
                    ));
                } else if !x.ty.is_printable() {
                    bad_ty(
                        self,
                        "supports Int, Float, Bool, Byte and String",
                        &x.ty,
                        args[0].value.span,
                    );
                }
                Self::mk(HK::Builtin(Builtin::Print, vec![x]), Ty::Void, span)
            }
            "assert" => {
                if !arity(self, 1) {
                    return Self::err_expr(span);
                }
                let x = self.expr(cx, &args[0].value, Some(&Ty::Bool));
                self.expect_ty(&Ty::Bool, &x.ty, args[0].value.span);
                Self::mk(HK::Builtin(Builtin::Assert, vec![x]), Ty::Void, span)
            }
            "assert_eq" => {
                if !arity(self, 2) {
                    return Self::err_expr(span);
                }
                let a = self.read_arg(cx, &args[0]);
                let b = self.read_arg(cx, &args[1]);
                self.expect_ty(&a.ty, &b.ty, args[1].value.span);
                if !a.ty.supports_eq(&self.records) {
                    bad_ty(
                        self,
                        "needs values that support `==`",
                        &a.ty,
                        args[0].value.span,
                    );
                }
                Self::mk(HK::Builtin(Builtin::AssertEq, vec![a, b]), Ty::Void, span)
            }
            "len" => {
                if !arity(self, 1) {
                    return Self::err_expr(span);
                }
                let x = self.read_arg(cx, &args[0]);
                if !matches!(x.ty, Ty::List(_) | Ty::Buffer | Ty::String | Ty::Error) {
                    bad_ty(
                        self,
                        "works on List, Buffer and String",
                        &x.ty,
                        args[0].value.span,
                    );
                }
                Self::mk(HK::Builtin(Builtin::Len, vec![x]), Ty::Int, span)
            }
            "append" => {
                if !arity(self, 2) {
                    return Self::err_expr(span);
                }
                let list = match &args[0].value.kind {
                    AK::Borrow(BorrowKind::Edit, p) => {
                        let pe = self.place(cx, p, "edit");
                        let ty = pe.ty.clone();
                        Self::mk(
                            HK::Borrow(BorrowKind::Edit, Box::new(pe)),
                            ty,
                            args[0].value.span,
                        )
                    }
                    _ => {
                        self.err(Diagnostic::error(
                            "B3017",
                            "`append` modifies its list; pass `edit <list>`",
                            args[0].value.span,
                        ));
                        return Self::err_expr(span);
                    }
                };
                let elem = match &list.ty {
                    Ty::List(t) => (**t).clone(),
                    Ty::Error => Ty::Error,
                    other => {
                        let o = other.clone();
                        bad_ty(
                            self,
                            "needs a List as its first argument",
                            &o,
                            args[0].value.span,
                        );
                        Ty::Error
                    }
                };
                let v = self.expr(cx, &args[1].value, Some(&elem));
                self.expect_ty(&elem, &v.ty, args[1].value.span);
                Self::mk(HK::Builtin(Builtin::Append, vec![list, v]), Ty::Void, span)
            }
            "clone" => {
                if !arity(self, 1) {
                    return Self::err_expr(span);
                }
                let x = self.read_arg(cx, &args[0]);
                if x.ty == Ty::Void {
                    self.err(Diagnostic::error(
                        "B2019",
                        "cannot clone a Void value",
                        args[0].value.span,
                    ));
                }
                let ty = x.ty.clone();
                Self::mk(HK::Builtin(Builtin::Clone, vec![x]), ty, span)
            }
            "is_ok" => {
                if !arity(self, 1) {
                    return Self::err_expr(span);
                }
                let x = self.read_arg(cx, &args[0]);
                if !matches!(x.ty, Ty::Result(_) | Ty::Error) {
                    bad_ty(self, "needs a Result", &x.ty, args[0].value.span);
                }
                Self::mk(HK::Builtin(Builtin::IsOk, vec![x]), Ty::Bool, span)
            }
            "unwrap" | "unwrap_error" => {
                if !arity(self, 1) {
                    return Self::err_expr(span);
                }
                let x = self.expr(cx, &args[0].value, None);
                let ty = match &x.ty {
                    Ty::Result(t) => {
                        if name == "unwrap" {
                            (**t).clone()
                        } else {
                            Ty::String
                        }
                    }
                    Ty::Error => Ty::Error,
                    other => {
                        let o = other.clone();
                        bad_ty(self, "needs a Result", &o, args[0].value.span);
                        Ty::Error
                    }
                };
                let b = if name == "unwrap" {
                    Builtin::Unwrap
                } else {
                    Builtin::UnwrapError
                };
                Self::mk(HK::Builtin(b, vec![x]), ty, span)
            }
            "ok" => {
                let want = match expected {
                    Some(Ty::Result(t)) => Some((**t).clone()),
                    _ => None,
                };
                if args.len() > 1 {
                    arity(self, 1);
                    return Self::err_expr(span);
                }
                if args.is_empty() {
                    if let Some(t) = &want {
                        if *t != Ty::Void && !t.is_error() {
                            let tn = self.ty_name(t);
                            self.err(Diagnostic::error(
                                "B2010",
                                format!("`ok()` without a value makes Result[Void], but Result[{tn}] is expected"),
                                span,
                            ));
                        }
                    }
                    return Self::mk(HK::ResultOk(None), Ty::Result(Box::new(Ty::Void)), span);
                }
                let v = self.expr(cx, &args[0].value, want.as_ref());
                if let Some(t) = &want {
                    self.expect_ty(t, &v.ty, args[0].value.span);
                }
                if v.ty == Ty::Void {
                    self.err(Diagnostic::error(
                        "B2019",
                        "use `ok()` for Result[Void]",
                        args[0].value.span,
                    ));
                }
                let ty = Ty::Result(Box::new(v.ty.clone()));
                Self::mk(HK::ResultOk(Some(Box::new(v))), ty, span)
            }
            "fail" => {
                if !arity(self, 1) {
                    return Self::err_expr(span);
                }
                let v = self.expr(cx, &args[0].value, Some(&Ty::String));
                self.expect_ty(&Ty::String, &v.ty, args[0].value.span);
                let ty = match expected {
                    Some(t @ Ty::Result(_)) => t.clone(),
                    _ => {
                        self.err(
                            Diagnostic::error(
                                "B2018",
                                "cannot infer the success type of `fail(...)`",
                                span,
                            )
                            .with_help("use it where a Result[T] is expected, e.g. `return fail(...)` or `let r: Result[Int] = fail(...)`"),
                        );
                        Ty::Error
                    }
                };
                Self::mk(HK::ResultFail(Box::new(v)), ty, span)
            }
            "Int" | "Float" | "Byte" => {
                if !arity(self, 1) {
                    return Self::err_expr(span);
                }
                let x = self.expr(cx, &args[0].value, None);
                let conv = match (name, &x.ty) {
                    ("Int", Ty::Float) => Some((Builtin::FloatToInt, Ty::Int)),
                    ("Int", Ty::Byte) => Some((Builtin::ByteToInt, Ty::Int)),
                    ("Float", Ty::Int) => Some((Builtin::IntToFloat, Ty::Float)),
                    ("Byte", Ty::Int) => Some((Builtin::IntToByte, Ty::Byte)),
                    (_, Ty::Error) => return Self::err_expr(span),
                    _ => None,
                };
                match conv {
                    Some((b, ty)) => Self::mk(HK::Builtin(b, vec![x]), ty, span),
                    None => {
                        let accepted = match name {
                            "Int" => "converts Float or Byte",
                            "Float" => "converts Int",
                            _ => "converts Int",
                        };
                        let o = x.ty.clone();
                        bad_ty(self, accepted, &o, args[0].value.span);
                        Self::err_expr(span)
                    }
                }
            }
            "Buffer" => {
                if args.len() != 1 || args[0].name.as_ref().map(|n| n.name.as_str()) != Some("size")
                {
                    self.err(
                        Diagnostic::error(
                            "B2010",
                            "`Buffer` takes exactly one named argument `size`",
                            span,
                        )
                        .with_help("write `Buffer(size=16)`"),
                    );
                    return Self::err_expr(span);
                }
                let n = self.expr(cx, &args[0].value, Some(&Ty::Int));
                self.expect_ty(&Ty::Int, &n.ty, args[0].value.span);
                Self::mk(HK::BufferNew(Box::new(n)), Ty::Buffer, span)
            }
            "range" => {
                self.err(Diagnostic::error(
                    "B2028",
                    "`range(...)` can only be used as the iterable of a `for` loop",
                    span,
                ));
                Self::err_expr(span)
            }
            _ => unreachable!(),
        }
    }

    // ----- phase 6: entry point and tests ------------------------------

    fn check_main(&mut self) -> Option<FnId> {
        if self.input.kind != ProgramKind::Bin {
            return None;
        }
        let m = self.loaded.entry_module?;
        let file = self.loaded.modules[m].file;
        match self.items[m].get("main").copied() {
            Some(Item::Fn(f)) => {
                let s = &self.sigs[f];
                if !s.params.is_empty() || s.ret != Ty::Void || s.public {
                    let span = s.span;
                    self.err(Diagnostic::error(
                        "B2020",
                        "the entry point must be declared exactly as `fn main():` (no parameters, no return type, not `pub`)",
                        span,
                    ));
                }
                Some(FnId(f))
            }
            _ => {
                self.err(Diagnostic::error(
                    "B2020",
                    "binary package has no `fn main():` in its entry module",
                    Span::new(file, 0, 0),
                ));
                None
            }
        }
    }

    fn collect_tests(&self) -> Vec<TestCase> {
        let mut out = Vec::new();
        for (i, s) in self.sigs.iter().enumerate() {
            if s.test.is_some() {
                let file = self
                    .loaded
                    .sources
                    .file(self.loaded.modules[s.module].file)
                    .display
                    .clone();
                out.push(TestCase {
                    name: s.name.clone(),
                    file,
                    func: FnId(i),
                });
            }
        }
        out
    }
}

fn const_ty(v: &ConstVal) -> Ty {
    match v {
        ConstVal::Int(_) => Ty::Int,
        ConstVal::Float(_) => Ty::Float,
        ConstVal::Bool(_) => Ty::Bool,
        ConstVal::Str(_) => Ty::String,
    }
}

/// Conservative "always returns" analysis for missing-return checking.
pub fn block_returns(b: &hir::Block) -> bool {
    b.stmts.iter().any(stmt_returns)
}

fn stmt_returns(s: &hir::Stmt) -> bool {
    use hir::StmtKind as SK;
    match &s.kind {
        SK::Return(_) => true,
        SK::If {
            then, els: Some(e), ..
        } => block_returns(then) && block_returns(e),
        SK::Scope(b) => block_returns(b),
        SK::While { cond, body } => {
            matches!(cond.kind, hir::ExprKind::Bool(true)) && !contains_break(body)
        }
        _ => false,
    }
}

/// Does `b` contain a `break` that targets the enclosing loop?
pub fn contains_break(b: &hir::Block) -> bool {
    use hir::StmtKind as SK;
    b.stmts.iter().any(|s| match &s.kind {
        SK::Break => true,
        SK::If { then, els, .. } => {
            contains_break(then) || els.as_ref().is_some_and(contains_break)
        }
        SK::Scope(b) => contains_break(b),
        // Nested loops own their breaks.
        _ => false,
    })
}
