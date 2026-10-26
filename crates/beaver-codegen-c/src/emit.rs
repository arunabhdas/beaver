//! MIR -> C11 translation unit.
//!
//! The generated code is a single translation unit. All evaluation order is
//! already explicit in the MIR (every operand is a constant or a local), so a
//! C statement never contains two side-effecting sub-expressions whose order
//! C leaves unspecified.

use beaver_mir::*;
use beaver_sema::hir::BinOp;
use beaver_sema::types::{RecordDef, Ty};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// `int main` calling the entry function.
    Executable,
    /// Test driver: `int main` dispatching to `test` blocks.
    Tests,
    /// No `main`; used to validate library packages.
    Library,
}

pub fn emit(prog: &MirProgram, output: Output) -> String {
    let mut e = Emitter {
        prog,
        records: &prog.records,
        types: BTreeSet::new(),
        locs: BTreeMap::new(),
        out: String::new(),
    };
    e.collect_types();
    e.emit_all(output);
    e.out
}

struct Emitter<'a> {
    prog: &'a MirProgram,
    records: &'a [RecordDef],
    types: BTreeSet<Ty>,
    locs: BTreeMap<Loc, usize>,
    out: String,
}

/// Injective, identifier-safe key for a type.
fn key(ty: &Ty, records: &[RecordDef]) -> String {
    match ty {
        Ty::Int => "i64".into(),
        Ty::Float => "f64".into(),
        Ty::Bool => "b".into(),
        Ty::Byte => "u8".into(),
        Ty::String => "str".into(),
        Ty::Buffer => "buf".into(),
        Ty::Void => "v".into(),
        Ty::Record(r) => {
            let m = &records[r.0].mangled;
            format!("R{}{}", m.len(), m)
        }
        Ty::List(t) => format!("L{}", key(t, records)),
        Ty::Result(t) => format!("E{}", key(t, records)),
        Ty::Error => unreachable!("error type reached the backend"),
    }
}

fn c_string_literal(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            // Avoid trigraphs and keep the output ASCII.
            b'?' => out.push_str("\\077"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{:03o}", b);
            }
        }
    }
    out.push('"');
    out
}

impl<'a> Emitter<'a> {
    fn cty(&self, ty: &Ty) -> String {
        match ty {
            Ty::Int => "int64_t".into(),
            Ty::Float => "double".into(),
            Ty::Bool => "bool".into(),
            Ty::Byte => "uint8_t".into(),
            Ty::String => "bvr_string".into(),
            Ty::Buffer => "bvr_buffer".into(),
            Ty::Void => "void".into(),
            Ty::Record(r) => self.records[r.0].mangled.clone(),
            Ty::List(t) => format!("bvr_list_{}", key(t, self.records)),
            Ty::Result(t) => format!("bvr_res_{}", key(t, self.records)),
            Ty::Error => unreachable!(),
        }
    }

    fn k(&self, ty: &Ty) -> String {
        key(ty, self.records)
    }

    fn is_copy(&self, ty: &Ty) -> bool {
        ty.is_copy(self.records)
    }

    // ----- type collection ---------------------------------------------

    fn add_type(&mut self, ty: &Ty) {
        if *ty == Ty::Error || !self.types.insert(ty.clone()) {
            return;
        }
        match ty {
            Ty::List(t) | Ty::Result(t) => self.add_type(&t.clone()),
            Ty::Record(r) => {
                for (_, f) in self.records[r.0].fields.clone() {
                    self.add_type(&f);
                }
            }
            _ => {}
        }
    }

    fn collect_types(&mut self) {
        for r in 0..self.records.len() {
            self.add_type(&Ty::Record(beaver_sema::types::RecordId(r)));
        }
        for f in &self.prog.functions {
            for l in &f.locals {
                self.add_type(&l.ty);
            }
            self.add_type(&f.ret);
            for b in &f.blocks {
                for s in &b.stmts {
                    if let Stmt::Assign(_, r) | Stmt::Eval(r) = s {
                        match r {
                            Rvalue::ListLit(t, _) => self.add_type(&Ty::List(Box::new(t.clone()))),
                            Rvalue::ResultOk(t, _) | Rvalue::ResultFail(t, _) => self.add_type(t),
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// Composite types in an order where by-value members come first.
    fn ordered_composites(&self) -> Vec<Ty> {
        let mut done = BTreeSet::new();
        let mut order = Vec::new();
        for t in &self.types {
            self.visit_order(t, &mut done, &mut order);
        }
        order
    }

    fn visit_order(&self, t: &Ty, done: &mut BTreeSet<Ty>, order: &mut Vec<Ty>) {
        if !matches!(t, Ty::Record(_) | Ty::List(_) | Ty::Result(_)) || done.contains(t) {
            return;
        }
        done.insert(t.clone());
        match t {
            Ty::Record(r) => {
                for (_, f) in &self.records[r.0].fields {
                    self.visit_order(f, done, order);
                }
            }
            Ty::Result(inner) => self.visit_order(inner, done, order),
            // A list header only holds a pointer to its elements.
            _ => {}
        }
        order.push(t.clone());
    }

    // ----- helpers -----------------------------------------------------

    fn drop_call(&self, ty: &Ty, ptr: &str) -> Option<String> {
        match ty {
            _ if self.is_copy(ty) || *ty == Ty::Void => None,
            Ty::String => Some(format!("bvr_string_drop({ptr})")),
            Ty::Buffer => Some(format!("bvr_buffer_drop({ptr})")),
            _ => Some(format!("bvr_drop_{}({ptr})", self.k(ty))),
        }
    }

    fn clone_expr(&self, ty: &Ty, ptr: &str) -> String {
        match ty {
            _ if self.is_copy(ty) => format!("(*{ptr})"),
            Ty::String => format!("bvr_string_clone({ptr})"),
            Ty::Buffer => format!("bvr_buffer_clone({ptr})"),
            _ => format!("bvr_clone_{}({ptr})", self.k(ty)),
        }
    }

    fn eq_expr(&self, ty: &Ty, a: &str, b: &str) -> String {
        match ty {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Byte => format!("((*{a}) == (*{b}))"),
            Ty::String => format!("bvr_string_eq({a}, {b})"),
            _ => format!("bvr_eq_{}({a}, {b})", self.k(ty)),
        }
    }

    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn emit_all(&mut self, output: Output) {
        self.line("/* Generated by the Beaver 0.1 compiler. Do not edit. */");
        self.line("#include \"beaver_rt.h\"");
        self.line("#include <string.h>");
        self.line("");
        let comps = self.ordered_composites();
        // Forward declarations.
        for t in &comps {
            let n = self.cty(t);
            self.line(&format!("typedef struct {n} {n};"));
        }
        self.line("");
        // Definitions.
        for t in &comps {
            self.emit_type_def(t);
        }
        // Helper prototypes then bodies.
        let mut protos = String::new();
        let mut bodies = String::new();
        for t in &comps {
            self.emit_helpers(t, &mut protos, &mut bodies);
        }
        self.out.push_str(&protos);
        self.line("");
        // Function prototypes.
        for f in &self.prog.functions {
            let sig = self.fn_signature(f);
            self.line(&format!("{sig};"));
        }
        self.line("");
        self.out.push_str(&bodies);
        // Functions (locations are collected while emitting bodies).
        let mut fn_text = String::new();
        for f in &self.prog.functions {
            fn_text.push_str(&self.emit_fn(f));
        }
        for (loc, id) in self.locs.clone() {
            self.line(&format!(
                "static const bvr_loc bvr_loc_{id} = {{{}, {}, {}}};",
                c_string_literal(&loc.file),
                loc.line,
                loc.col
            ));
        }
        self.line("");
        self.out.push_str(&fn_text);
        match output {
            Output::Executable => {
                let entry =
                    &self.prog.functions[self.prog.entry.expect("binary has an entry point")];
                self.line("int main(int argc, char **argv) {");
                self.line("    bvr_rt_init(argc, argv);");
                self.line(&format!("    {}();", entry.mangled));
                self.line("    return bvr_rt_finish();");
                self.line("}");
            }
            Output::Tests => {
                let n = self.prog.tests.len();
                if n == 0 {
                    self.line("static const char *const bvr_test_names[1] = {NULL};");
                    self.line("static const bvr_test_fn bvr_test_fns[1] = {NULL};");
                } else {
                    let names: Vec<String> = self
                        .prog
                        .tests
                        .iter()
                        .map(|t| c_string_literal(&t.name))
                        .collect();
                    let fns: Vec<String> = self
                        .prog
                        .tests
                        .iter()
                        .map(|t| self.prog.functions[t.func].mangled.clone())
                        .collect();
                    self.line(&format!(
                        "static const char *const bvr_test_names[{n}] = {{{}}};",
                        names.join(", ")
                    ));
                    self.line(&format!(
                        "static const bvr_test_fn bvr_test_fns[{n}] = {{{}}};",
                        fns.join(", ")
                    ));
                }
                self.line("int main(int argc, char **argv) {");
                self.line("    bvr_rt_init(argc, argv);");
                self.line(&format!(
                    "    return bvr_test_main(argc, argv, {n}, bvr_test_names, bvr_test_fns);"
                ));
                self.line("}");
            }
            Output::Library => {}
        }
    }

    fn emit_type_def(&mut self, t: &Ty) {
        let n = self.cty(t);
        match t {
            Ty::Record(r) => {
                self.line(&format!("struct {n} {{"));
                for (fname, fty) in self.records[r.0].fields.clone() {
                    let ct = self.cty(&fty);
                    self.line(&format!("    {ct} f_{fname};"));
                }
                self.line("};");
            }
            Ty::List(e) => {
                let et = self.cty(e);
                self.line(&format!(
                    "struct {n} {{ {et} *ptr; int64_t len; int64_t cap; }};"
                ));
            }
            Ty::Result(inner) => {
                if **inner == Ty::Void {
                    self.line(&format!("struct {n} {{ bool ok; bvr_string err; }};"));
                } else {
                    let it = self.cty(inner);
                    self.line(&format!(
                        "struct {n} {{ bool ok; union {{ {it} val; bvr_string err; }} u; }};"
                    ));
                }
            }
            _ => {}
        }
        self.line("");
    }

    fn emit_helpers(&self, t: &Ty, protos: &mut String, bodies: &mut String) {
        let n = self.cty(t);
        let k = self.k(t);
        let mut def = |proto: String, body: String| {
            let _ = writeln!(protos, "{proto};");
            let _ = writeln!(bodies, "{proto} {{\n{body}}}\n");
        };
        match t {
            Ty::Record(r) => {
                let rec = &self.records[r.0];
                if !rec.is_copy {
                    let mut body = String::new();
                    for (fname, fty) in rec.fields.iter().rev() {
                        if let Some(c) = self.drop_call(fty, &format!("&p->f_{fname}")) {
                            let _ = writeln!(body, "    {c};");
                        }
                    }
                    def(format!("void bvr_drop_{k}({n} *p)"), body);
                    let mut body = format!("    {n} r;\n    memset(&r, 0, sizeof r);\n");
                    for (fname, fty) in &rec.fields {
                        let c = self.clone_expr(fty, &format!("&p->f_{fname}"));
                        let _ = writeln!(body, "    r.f_{fname} = {c};");
                    }
                    body.push_str("    return r;\n");
                    def(format!("{n} bvr_clone_{k}({n} *p)"), body);
                }
                if rec.supports_eq {
                    let parts: Vec<String> = rec
                        .fields
                        .iter()
                        .map(|(f, ft)| {
                            self.eq_expr(ft, &format!("&a->f_{f}"), &format!("&b->f_{f}"))
                        })
                        .collect();
                    def(
                        format!("bool bvr_eq_{k}({n} *a, {n} *b)"),
                        format!("    return {};\n", parts.join(" && ")),
                    );
                }
            }
            Ty::List(e) => {
                let et = self.cty(e);
                let ek = self.k(e);
                def(
                    format!("{n} bvr_list_{ek}_with_cap(int64_t cap)"),
                    format!(
                        "    {n} l;\n    l.len = 0;\n    l.cap = cap;\n    l.ptr = cap > 0 ? ({et} *)bvr_alloc_array((int64_t)sizeof({et}), cap, NULL) : NULL;\n    return l;\n"
                    ),
                );
                def(
                    format!("void bvr_list_{ek}_push({n} *l, {et} v)"),
                    format!(
                        "    if (l->len == l->cap) {{\n        l->ptr = ({et} *)bvr_grow_array(l->ptr, (int64_t)sizeof({et}), &l->cap, NULL);\n    }}\n    l->ptr[l->len] = v;\n    l->len++;\n"
                    ),
                );
                def(
                    format!("{et} *bvr_list_{ek}_at({n} *l, int64_t i, const bvr_loc *loc)"),
                    "    bvr_bounds_check(i, l->len, loc);\n    return &l->ptr[i];\n".to_string(),
                );
                let mut body = String::new();
                if let Some(c) = self.drop_call(e, "&l->ptr[i]") {
                    let _ = writeln!(
                        body,
                        "    for (int64_t i = 0; i < l->len; i++) {{\n        {c};\n    }}"
                    );
                }
                body.push_str(
                    "    bvr_free(l->ptr);\n    l->ptr = NULL;\n    l->len = 0;\n    l->cap = 0;\n",
                );
                def(format!("void bvr_drop_{k}({n} *l)"), body);
                let c = self.clone_expr(e, "&l->ptr[i]");
                def(
                    format!("{n} bvr_clone_{k}({n} *l)"),
                    format!(
                        "    {n} r = bvr_list_{ek}_with_cap(l->len);\n    for (int64_t i = 0; i < l->len; i++) {{\n        r.ptr[i] = {c};\n        r.len = i + 1;\n    }}\n    return r;\n"
                    ),
                );
            }
            Ty::Result(inner) => {
                let ik = self.k(inner);
                if **inner == Ty::Void {
                    def(
                        format!("void bvr_drop_{k}({n} *r)"),
                        "    if (!r->ok) {\n        bvr_string_drop(&r->err);\n    }\n".to_string(),
                    );
                    def(
                        format!("{n} bvr_clone_{k}({n} *r)"),
                        format!("    {n} o;\n    memset(&o, 0, sizeof o);\n    o.ok = r->ok;\n    if (!r->ok) {{\n        o.err = bvr_string_clone(&r->err);\n    }}\n    return o;\n"),
                    );
                    def(
                        format!("void bvr_res_{ik}_unwrap({n} r, const bvr_loc *loc)"),
                        "    if (!r.ok) {\n        bvr_unwrap_failed(&r.err, loc);\n    }\n"
                            .to_string(),
                    );
                    def(
                        format!("bvr_string bvr_res_{ik}_unwrap_err({n} r, const bvr_loc *loc)"),
                        "    if (r.ok) {\n        bvr_unwrap_error_failed(loc);\n    }\n    return r.err;\n".to_string(),
                    );
                } else {
                    let it = self.cty(inner);
                    let mut body = String::from("    if (r->ok) {\n");
                    if let Some(c) = self.drop_call(inner, "&r->u.val") {
                        let _ = writeln!(body, "        {c};");
                    }
                    body.push_str("    } else {\n        bvr_string_drop(&r->u.err);\n    }\n");
                    def(format!("void bvr_drop_{k}({n} *r)"), body);
                    let c = self.clone_expr(inner, "&r->u.val");
                    def(
                        format!("{n} bvr_clone_{k}({n} *r)"),
                        format!("    {n} o;\n    memset(&o, 0, sizeof o);\n    o.ok = r->ok;\n    if (r->ok) {{\n        o.u.val = {c};\n    }} else {{\n        o.u.err = bvr_string_clone(&r->u.err);\n    }}\n    return o;\n"),
                    );
                    def(
                        format!("{it} bvr_res_{ik}_unwrap({n} r, const bvr_loc *loc)"),
                        "    if (!r.ok) {\n        bvr_unwrap_failed(&r.u.err, loc);\n    }\n    return r.u.val;\n".to_string(),
                    );
                    let mut body = String::from("    if (r.ok) {\n");
                    if let Some(c) = self.drop_call(inner, "&r.u.val") {
                        let _ = writeln!(body, "        {c};");
                    }
                    body.push_str(
                        "        bvr_unwrap_error_failed(loc);\n    }\n    return r.u.err;\n",
                    );
                    def(
                        format!("bvr_string bvr_res_{ik}_unwrap_err({n} r, const bvr_loc *loc)"),
                        body,
                    );
                }
            }
            _ => {}
        }
    }

    fn fn_signature(&self, f: &MirFn) -> String {
        let ret = self.cty(&f.ret);
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                let l = &f.locals[p.0];
                let t = self.cty(&l.ty);
                if l.is_ref {
                    format!("{t} *l{}", p.0)
                } else {
                    format!("{t} l{}", p.0)
                }
            })
            .collect();
        let params = if params.is_empty() {
            "void".to_string()
        } else {
            params.join(", ")
        };
        format!("{ret} {}({params})", f.mangled)
    }

    // ----- function bodies ---------------------------------------------

    fn loc_ref(&mut self, l: &Loc) -> String {
        let n = self.locs.len();
        let id = *self.locs.entry(l.clone()).or_insert(n);
        format!("&bvr_loc_{id}")
    }

    fn place(&mut self, f: &MirFn, p: &Place) -> String {
        let l = &f.locals[p.local.0];
        let mut s = if l.is_ref {
            format!("(*l{})", p.local.0)
        } else {
            format!("l{}", p.local.0)
        };
        let mut ty = l.ty.clone();
        for pr in &p.proj {
            match pr {
                Proj::Field(i) => {
                    let Ty::Record(r) = &ty else { unreachable!() };
                    let (fname, fty) = self.records[r.0].fields[*i].clone();
                    s = format!("{s}.f_{fname}");
                    ty = fty;
                }
                Proj::Index(idx, loc) => {
                    let lr = self.loc_ref(loc);
                    match &ty {
                        Ty::List(e) => {
                            let ek = self.k(e);
                            s = format!("(*bvr_list_{ek}_at(&{s}, l{}, {lr}))", idx.0);
                            ty = (**e).clone();
                        }
                        Ty::Buffer => {
                            s = format!("(*bvr_buffer_at(&{s}, l{}, {lr}))", idx.0);
                            ty = Ty::Byte;
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
        s
    }

    fn operand(&mut self, f: &MirFn, o: &Operand) -> String {
        match o {
            Operand::Copy(p) | Operand::Move(p) => self.place(f, p),
            Operand::Ref(l) => format!("l{}", l.0),
            Operand::Const(c) => match c {
                Const::Int(v) if *v == i64::MIN => "INT64_MIN".into(),
                Const::Int(v) => format!("INT64_C({v})"),
                Const::Float(v) => {
                    let s = format!("{v:?}");
                    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN")
                    {
                        s
                    } else {
                        format!("{s}.0")
                    }
                }
                Const::Bool(b) => (if *b { "true" } else { "false" }).into(),
                Const::Str(s) => format!("bvr_str_lit({}, {})", c_string_literal(s), s.len()),
            },
        }
    }

    fn rvalue(&mut self, f: &MirFn, r: &Rvalue) -> String {
        match r {
            Rvalue::Use(o) => self.operand(f, o),
            Rvalue::CheckedInt(op, a, b, loc) => {
                let name = match op {
                    IntOp::Add => "add",
                    IntOp::Sub => "sub",
                    IntOp::Mul => "mul",
                    IntOp::Div => "div",
                    IntOp::Rem => "rem",
                };
                let (a, b, l) = (self.operand(f, a), self.operand(f, b), self.loc_ref(loc));
                format!("bvr_{name}_i64({a}, {b}, {l})")
            }
            Rvalue::CheckedNeg(a, loc) => {
                let (a, l) = (self.operand(f, a), self.loc_ref(loc));
                format!("bvr_neg_i64({a}, {l})")
            }
            Rvalue::Float(op, a, b) => {
                let sym = match op {
                    FloatOp::Add => "+",
                    FloatOp::Sub => "-",
                    FloatOp::Mul => "*",
                    FloatOp::Div => "/",
                };
                let (a, b) = (self.operand(f, a), self.operand(f, b));
                format!("({a} {sym} {b})")
            }
            Rvalue::FloatNeg(a) => format!("(-{})", self.operand(f, a)),
            Rvalue::Not(a) => format!("(!{})", self.operand(f, a)),
            Rvalue::Compare(op, a, b) => {
                let sym = match op {
                    BinOp::Eq => "==",
                    BinOp::Ne => "!=",
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                    _ => unreachable!(),
                };
                let (a, b) = (self.operand(f, a), self.operand(f, b));
                format!("({a} {sym} {b})")
            }
            Rvalue::Equal { ty, a, b, negate } => {
                let (pa, pb) = (self.place(f, a), self.place(f, b));
                let e = self.eq_expr(ty, &format!("&{pa}"), &format!("&{pb}"));
                if *negate {
                    format!("(!{e})")
                } else {
                    e
                }
            }
            Rvalue::Concat(a, b) => {
                let (pa, pb) = (self.place(f, a), self.place(f, b));
                format!("bvr_string_concat(&{pa}, &{pb})")
            }
            Rvalue::Construct(rid, ops) => {
                let rec = &self.records[rid.0];
                let n = rec.mangled.clone();
                let names: Vec<String> = rec.fields.iter().map(|(f, _)| f.clone()).collect();
                let parts: Vec<String> = ops
                    .iter()
                    .zip(names)
                    .map(|(o, fname)| format!(".f_{fname} = {}", self.operand(f, o)))
                    .collect();
                format!("(({n}){{{}}})", parts.join(", "))
            }
            Rvalue::BufferNew(o, loc) => {
                let (o, l) = (self.operand(f, o), self.loc_ref(loc));
                format!("bvr_buffer_new({o}, {l})")
            }
            Rvalue::ResultOk(ty, o) => {
                let n = self.cty(ty);
                match o {
                    Some(o) => format!("(({n}){{.ok = true, .u.val = {}}})", self.operand(f, o)),
                    None => format!("(({n}){{.ok = true}})"),
                }
            }
            Rvalue::ResultFail(ty, o) => {
                let n = self.cty(ty);
                let inner_void = matches!(ty, Ty::Result(t) if **t == Ty::Void);
                let v = self.operand(f, o);
                if inner_void {
                    format!("(({n}){{.ok = false, .err = {v}}})")
                } else {
                    format!("(({n}){{.ok = false, .u.err = {v}}})")
                }
            }
            Rvalue::Call(id, ops) => {
                let callee = &self.prog.functions[*id];
                let name = callee.mangled.clone();
                let args: Vec<String> = ops.iter().map(|o| self.operand(f, o)).collect();
                format!("{name}({})", args.join(", "))
            }
            Rvalue::Clone(p) => {
                let ty = f.place_ty(p, self.records);
                let s = self.place(f, p);
                self.clone_expr(&ty, &format!("&{s}"))
            }
            Rvalue::Len(p) => format!("({}).len", self.place(f, p)),
            Rvalue::IsOk(p) => format!("({}).ok", self.place(f, p)),
            Rvalue::Unwrap(o, loc) | Rvalue::UnwrapError(o, loc) => {
                let ty = f.operand_ty(o, self.records);
                let Ty::Result(inner) = &ty else {
                    unreachable!()
                };
                let ik = self.k(inner);
                let (v, l) = (self.operand(f, o), self.loc_ref(loc));
                let suffix = if matches!(r, Rvalue::Unwrap(..)) {
                    ""
                } else {
                    "_err"
                };
                format!("bvr_res_{ik}_unwrap{suffix}({v}, {l})")
            }
            Rvalue::ResultTakeOk(p) => format!("({}).u.val", self.place(f, p)),
            Rvalue::ResultTakeErr(p) => {
                let ty = f.place_ty(p, self.records);
                let s = self.place(f, p);
                if matches!(&ty, Ty::Result(t) if **t == Ty::Void) {
                    format!("({s}).err")
                } else {
                    format!("({s}).u.err")
                }
            }
            Rvalue::IntToFloat(o) => format!("((double){})", self.operand(f, o)),
            Rvalue::FloatToInt(o, loc) => {
                let (v, l) = (self.operand(f, o), self.loc_ref(loc));
                format!("bvr_f64_to_i64({v}, {l})")
            }
            Rvalue::IntToByte(o, loc) => {
                let (v, l) = (self.operand(f, o), self.loc_ref(loc));
                format!("bvr_i64_to_u8({v}, {l})")
            }
            Rvalue::ByteToInt(o) => format!("((int64_t){})", self.operand(f, o)),
            Rvalue::ListLit(..) => unreachable!("list literals are emitted as statements"),
        }
    }

    fn emit_fn(&mut self, f: &MirFn) -> String {
        let mut o = String::new();
        let _ = writeln!(o, "{} {{", self.fn_signature(f));
        let params: BTreeSet<usize> = f.params.iter().map(|p| p.0).collect();
        for (i, l) in f.locals.iter().enumerate() {
            if l.ty == Ty::Void || l.ty == Ty::Error {
                continue;
            }
            if !params.contains(&i) {
                let t = self.cty(&l.ty);
                if l.is_ref {
                    let _ = writeln!(o, "    {t} *l{i} = NULL;");
                } else if matches!(l.ty, Ty::Int | Ty::Float | Ty::Bool | Ty::Byte) {
                    let _ = writeln!(o, "    {t} l{i} = 0;");
                } else {
                    let _ = writeln!(o, "    {t} l{i};");
                    let _ = writeln!(o, "    memset(&l{i}, 0, sizeof l{i});");
                }
            }
            let _ = writeln!(o, "    (void)l{i};");
        }
        // Reachability and goto targets.
        let n = f.blocks.len();
        let mut reachable = vec![false; n];
        let mut targets: BTreeSet<usize> = BTreeSet::new();
        let mut stack = vec![0usize];
        while let Some(b) = stack.pop() {
            if reachable[b] {
                continue;
            }
            reachable[b] = true;
            let succ: Vec<Bb> = match &f.blocks[b].term {
                Terminator::Goto(t) => vec![*t],
                Terminator::Branch(_, t, e) => vec![*t, *e],
                _ => vec![],
            };
            for s in succ {
                targets.insert(s.0);
                stack.push(s.0);
            }
        }
        // Emit blocks in index order; every block ends in an explicit jump.
        for (b, _) in reachable.iter().enumerate().filter(|(_, r)| **r) {
            if targets.contains(&b) {
                let _ = writeln!(o, "bb{b}:;");
            }
            for s in &f.blocks[b].stmts {
                let text = self.stmt(f, s);
                if !text.is_empty() {
                    o.push_str(&text);
                }
            }
            let term = match &f.blocks[b].term {
                Terminator::Goto(t) => format!("    goto bb{};\n", t.0),
                Terminator::Branch(c, t, e) => {
                    let c = self.operand(f, c);
                    format!("    if ({c}) goto bb{}; else goto bb{};\n", t.0, e.0)
                }
                Terminator::Return => match f.ret_local {
                    Some(r) => format!("    return l{};\n", r.0),
                    None => "    return;\n".to_string(),
                },
                Terminator::Unreachable => "    bvr_unreachable();\n".to_string(),
            };
            o.push_str(&term);
        }
        o.push_str("}\n\n");
        o
    }

    fn stmt(&mut self, f: &MirFn, s: &Stmt) -> String {
        match s {
            Stmt::Assign(p, Rvalue::ListLit(elem, ops)) => {
                let ek = self.k(elem);
                let dst = self.place(f, p);
                let mut o = format!("    {dst} = bvr_list_{ek}_with_cap({});\n", ops.len());
                for op in ops {
                    let v = self.operand(f, op);
                    let dst = self.place(f, p);
                    let _ = writeln!(o, "    bvr_list_{ek}_push(&{dst}, {v});");
                }
                o
            }
            Stmt::Assign(p, r) => {
                let ty = f.place_ty(p, self.records);
                let rv = self.rvalue(f, r);
                if ty == Ty::Void {
                    return format!("    {rv};\n");
                }
                let dst = self.place(f, p);
                format!("    {dst} = {rv};\n")
            }
            Stmt::BindRef(l, p) => {
                let target = self.place(f, p);
                format!("    l{} = &{target};\n", l.0)
            }
            Stmt::Eval(r) => format!("    {};\n", self.rvalue(f, r)),
            Stmt::Drop(p) => {
                let ty = f.place_ty(p, self.records);
                let s = self.place(f, p);
                match self.drop_call(&ty, &format!("&{s}")) {
                    Some(c) => format!("    {c};\n"),
                    None => String::new(),
                }
            }
            Stmt::EndBorrow(l) => format!("    /* end borrow l{} */\n", l.0),
            Stmt::Append(p, v) => {
                let ty = f.place_ty(p, self.records);
                let Ty::List(e) = &ty else { unreachable!() };
                let ek = self.k(e);
                let (dst, val) = (self.place(f, p), self.operand(f, v));
                format!("    bvr_list_{ek}_push(&{dst}, {val});\n")
            }
            Stmt::Print(p) => {
                let ty = f.place_ty(p, self.records);
                let s = self.place(f, p);
                match ty {
                    Ty::Int => format!("    bvr_print_i64({s});\n"),
                    Ty::Float => format!("    bvr_print_f64({s});\n"),
                    Ty::Bool => format!("    bvr_print_bool({s});\n"),
                    Ty::Byte => format!("    bvr_print_u8({s});\n"),
                    Ty::String => format!("    bvr_print_string(&{s});\n"),
                    _ => unreachable!("type checker restricts print"),
                }
            }
            Stmt::Assert(c, loc) => {
                let (c, l) = (self.operand(f, c), self.loc_ref(loc));
                format!("    bvr_assert({c}, {l});\n")
            }
            Stmt::AssertEq(a, b, loc) => {
                let ty = f.place_ty(a, self.records);
                let (pa, pb, l) = (self.place(f, a), self.place(f, b), self.loc_ref(loc));
                match ty {
                    Ty::Int => format!("    bvr_assert_eq_i64({pa}, {pb}, {l});\n"),
                    Ty::Float => format!("    bvr_assert_eq_f64({pa}, {pb}, {l});\n"),
                    Ty::Bool => format!("    bvr_assert_eq_bool({pa}, {pb}, {l});\n"),
                    Ty::Byte => format!("    bvr_assert_eq_u8({pa}, {pb}, {l});\n"),
                    Ty::String => format!("    bvr_assert_eq_string(&{pa}, &{pb}, {l});\n"),
                    _ => {
                        let e = self.eq_expr(&ty, &format!("&{pa}"), &format!("&{pb}"));
                        format!("    if (!{e}) {{\n        bvr_assert_eq_failed({l});\n    }}\n")
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_literals_are_escaped_safely() {
        assert_eq!(c_string_literal("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(c_string_literal("??="), "\"\\077\\077=\"");
        assert_eq!(c_string_literal("\0\n"), "\"\\000\\012\"");
        assert_eq!(c_string_literal("é"), "\"\\303\\251\"");
    }

    #[test]
    fn type_keys_are_distinct() {
        let recs: Vec<RecordDef> = Vec::new();
        let tys = [
            Ty::List(Box::new(Ty::Bool)),
            Ty::List(Box::new(Ty::Buffer)),
            Ty::Result(Box::new(Ty::List(Box::new(Ty::String)))),
            Ty::List(Box::new(Ty::Result(Box::new(Ty::String)))),
        ];
        let keys: BTreeSet<String> = tys.iter().map(|t| key(t, &recs)).collect();
        assert_eq!(keys.len(), tys.len());
    }
}
