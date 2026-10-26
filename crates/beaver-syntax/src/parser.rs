//! Recursive-descent parser producing [`crate::ast::Module`].

use crate::ast::*;
use crate::token::{Tok, Token};
use beaver_source::{Diagnostic, Span};

pub fn parse(tokens: &[Token]) -> Result<Module, Vec<Diagnostic>> {
    let mut p = Parser {
        toks: tokens,
        pos: 0,
        errors: Vec::new(),
    };
    let module = p.module();
    if p.errors.is_empty() {
        Ok(module)
    } else {
        Err(p.errors)
    }
}

struct Parser<'t> {
    toks: &'t [Token],
    pos: usize,
    errors: Vec<Diagnostic>,
}

type PResult<T> = Result<T, Diagnostic>;

/// Words from other languages that we diagnose specially.
const UNSUPPORTED: &[(&str, &str)] = &[
    (
        "class",
        "classes are not part of Beaver 0.1; use a data-only `type` record",
    ),
    (
        "share",
        "`share` (shared ownership) is not available in Beaver 0.1",
    ),
    ("unsafe", "`unsafe` code is not available in Beaver 0.1"),
    ("def", "functions are declared with `fn`"),
    ("import", "modules are imported with `use`"),
    ("match", "pattern matching is not available in Beaver 0.1"),
    ("enum", "enum declarations are not available in Beaver 0.1"),
    ("struct", "records are declared with `type Name:`"),
    (
        "lambda",
        "closures and lambdas are not available in Beaver 0.1",
    ),
    ("async", "async code is not available in Beaver 0.1"),
    ("await", "async code is not available in Beaver 0.1"),
    ("impl", "methods are not available in Beaver 0.1"),
    ("trait", "traits are not available in Beaver 0.1"),
];

impl<'t> Parser<'t> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> &Token {
        let t = &self.toks[self.pos];
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn unexpected(&self, expected: &str) -> Diagnostic {
        let tok = self.peek();
        if let Tok::Ident(name) = tok {
            if let Some((_, msg)) = UNSUPPORTED.iter().find(|(w, _)| w == name) {
                return Diagnostic::error("B1017", msg.to_string(), self.span());
            }
        }
        let d = Diagnostic::error(
            "B1012",
            format!("expected {expected}, found {}", tok.describe()),
            self.span(),
        );
        match tok {
            Tok::Indent => Diagnostic::error("B1005", "unexpected indentation", self.span())
                .with_help("indentation only increases after a line ending in `:`"),
            _ => d,
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> PResult<Span> {
        if self.at(t) {
            Ok(self.bump().span)
        } else {
            Err(self.unexpected(what))
        }
    }

    fn ident(&mut self, what: &str) -> PResult<Ident> {
        if let Tok::Ident(name) = self.peek() {
            let name = name.clone();
            let span = self.bump().span;
            Ok(Ident { name, span })
        } else if self.peek().is_keyword() {
            Err(Diagnostic::error(
                "B1012",
                format!(
                    "expected {what}, found reserved word `{}`",
                    self.peek().text()
                ),
                self.span(),
            ))
        } else {
            Err(self.unexpected(what))
        }
    }

    fn end_of_line(&mut self) -> PResult<()> {
        if self.eat(&Tok::Newline) {
            Ok(())
        } else {
            Err(self.unexpected("end of line"))
        }
    }

    // ----- items -------------------------------------------------------

    fn module(&mut self) -> Module {
        let mut items = Vec::new();
        loop {
            while self.eat(&Tok::Newline) {}
            if self.at(&Tok::Eof) {
                break;
            }
            match self.item() {
                Ok(item) => items.push(item),
                Err(d) => {
                    self.errors.push(d);
                    self.recover_to_item();
                }
            }
        }
        Module { items }
    }

    /// Skip tokens until the start of the next top-level line.
    fn recover_to_item(&mut self) {
        let mut depth: i32 = 0;
        loop {
            match self.peek() {
                Tok::Eof => return,
                Tok::Indent => depth += 1,
                Tok::Dedent => depth -= 1,
                Tok::Newline if depth <= 0 => {
                    self.bump();
                    if !matches!(self.peek(), Tok::Indent) {
                        return;
                    }
                    continue;
                }
                _ => {}
            }
            self.bump();
            if depth <= 0
                && matches!(
                    self.peek(),
                    Tok::Fn | Tok::Type | Tok::Pub | Tok::Const | Tok::Use | Tok::Test
                )
                && matches!(self.toks[self.pos - 1].tok, Tok::Newline | Tok::Dedent)
            {
                return;
            }
        }
    }

    fn item(&mut self) -> PResult<Item> {
        let start = self.span();
        let public = self.eat(&Tok::Pub);
        match self.peek() {
            Tok::Fn => self.fn_decl(public, start).map(Item::Fn),
            Tok::Type => self.type_decl(public, start).map(Item::Type),
            Tok::Const => self.const_decl(public, start).map(Item::Const),
            Tok::Use if !public => {
                self.bump();
                let mut path = vec![self.ident("module name")?];
                while self.eat(&Tok::Dot) {
                    path.push(self.ident("module name")?);
                }
                let span = start.to(self.prev_span());
                self.end_of_line()?;
                Ok(Item::Use(UseDecl { path, span }))
            }
            Tok::Test if !public => {
                self.bump();
                let name_span = self.span();
                let name = match self.peek() {
                    Tok::Str(s) => s.clone(),
                    _ => return Err(self.unexpected("test name string")),
                };
                self.bump();
                let body = self.suite()?;
                let span = start.to(name_span);
                Ok(Item::Test(TestDecl {
                    name,
                    name_span,
                    body,
                    span,
                }))
            }
            Tok::Use | Tok::Test => Err(Diagnostic::error(
                "B1012",
                format!("`pub` cannot be applied to `{}`", self.peek().text()),
                start,
            )),
            Tok::Let | Tok::Var => Err(Diagnostic::error(
                "B1012",
                "statements are not allowed at the top level of a module",
                self.span(),
            )
            .with_help("use `const NAME: Type = value` for module-level constants, or move this into a function")),
            _ => Err(self.unexpected("`fn`, `type`, `const`, `use` or `test`")),
        }
    }

    fn fn_decl(&mut self, public: bool, start: Span) -> PResult<FnDecl> {
        self.expect(&Tok::Fn, "`fn`")?;
        let name = self.ident("function name")?;
        self.expect(&Tok::LParen, "`(`")?;
        let mut params = Vec::new();
        if !self.at(&Tok::RParen) {
            loop {
                let pname = self.ident("parameter name")?;
                self.expect(&Tok::Colon, "`:` and a parameter type")?;
                let mode = match self.peek() {
                    Tok::View => {
                        self.bump();
                        ParamMode::View
                    }
                    Tok::Edit => {
                        self.bump();
                        ParamMode::Edit
                    }
                    Tok::Take => {
                        self.bump();
                        ParamMode::Take
                    }
                    _ => ParamMode::Default,
                };
                let ty = self.type_expr()?;
                params.push(Param {
                    name: pname,
                    mode,
                    ty,
                });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen, "`)`")?;
        let ret = if self.eat(&Tok::Arrow) {
            Some(self.type_expr()?)
        } else {
            None
        };
        let span = start.to(self.prev_span());
        let body = self.suite()?;
        Ok(FnDecl {
            public,
            name,
            params,
            ret,
            body,
            span,
        })
    }

    fn type_decl(&mut self, public: bool, start: Span) -> PResult<TypeDecl> {
        self.expect(&Tok::Type, "`type`")?;
        let name = self.ident("type name")?;
        let span = start.to(self.prev_span());
        self.expect(&Tok::Colon, "`:`")?;
        self.end_of_line()?;
        if !self.eat(&Tok::Indent) {
            return Err(Diagnostic::error(
                "B1006",
                "expected an indented list of fields",
                self.span(),
            ));
        }
        let mut fields = Vec::new();
        while !self.at(&Tok::Dedent) && !self.at(&Tok::Eof) {
            let fname = self.ident("field name")?;
            self.expect(&Tok::Colon, "`:` and a field type")?;
            let ty = self.type_expr()?;
            self.end_of_line()?;
            fields.push(FieldDecl { name: fname, ty });
        }
        self.eat(&Tok::Dedent);
        Ok(TypeDecl {
            public,
            name,
            fields,
            span,
        })
    }

    fn const_decl(&mut self, public: bool, start: Span) -> PResult<ConstDecl> {
        self.expect(&Tok::Const, "`const`")?;
        let name = self.ident("constant name")?;
        if !self.at(&Tok::Colon) {
            return Err(Diagnostic::error(
                "B1012",
                "constants require an explicit type",
                self.span(),
            )
            .with_help(format!("write `const {}: Int = ...`", name.name)));
        }
        self.bump();
        let ty = self.type_expr()?;
        self.expect(&Tok::Assign, "`=`")?;
        let value = self.expr()?;
        let span = start.to(self.prev_span());
        self.end_of_line()?;
        Ok(ConstDecl {
            public,
            name,
            ty,
            value,
            span,
        })
    }

    fn type_expr(&mut self) -> PResult<TypeExpr> {
        let first = self.ident("type")?;
        let start = first.span;
        let mut path = vec![first];
        while self.eat(&Tok::Dot) {
            path.push(self.ident("type name")?);
        }
        let arg = if self.eat(&Tok::LBracket) {
            let a = self.type_expr()?;
            self.expect(&Tok::RBracket, "`]`")?;
            Some(Box::new(a))
        } else {
            None
        };
        Ok(TypeExpr {
            path,
            arg,
            span: start.to(self.prev_span()),
        })
    }

    // ----- statements --------------------------------------------------

    fn suite(&mut self) -> PResult<Block> {
        self.expect(&Tok::Colon, "`:`")?;
        if !self.at(&Tok::Newline) {
            return Err(Diagnostic::error(
                "B1006",
                "a block must start on a new, indented line after `:`",
                self.span(),
            ));
        }
        self.bump();
        let start = self.span();
        if !self.eat(&Tok::Indent) {
            return Err(
                Diagnostic::error("B1006", "expected an indented block", self.span())
                    .with_help("indent the block body by 4 spaces; use `pass` for an empty block"),
            );
        }
        let mut stmts = Vec::new();
        while !self.at(&Tok::Dedent) && !self.at(&Tok::Eof) {
            stmts.push(self.stmt()?);
        }
        let end = self.prev_span();
        self.eat(&Tok::Dedent);
        Ok(Block {
            stmts,
            span: start.to(end),
        })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let kind = match self.peek().clone() {
            Tok::Let | Tok::Var => {
                let mutable = self.at(&Tok::Var);
                self.bump();
                let name = self.ident("variable name")?;
                let ty = if self.eat(&Tok::Colon) {
                    Some(self.type_expr()?)
                } else {
                    None
                };
                if !self.at(&Tok::Assign) {
                    return Err(self.unexpected("`=` (variables must be initialized)"));
                }
                self.bump();
                let init = self.expr()?;
                self.end_of_line()?;
                StmtKind::Let {
                    mutable,
                    name,
                    ty,
                    init,
                }
            }
            Tok::Return => {
                self.bump();
                let value = if self.at(&Tok::Newline) {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.end_of_line()?;
                StmtKind::Return(value)
            }
            Tok::If => {
                self.bump();
                return self.if_rest(start);
            }
            Tok::Elif | Tok::Else => {
                return Err(Diagnostic::error(
                    "B1012",
                    format!("`{}` without a matching `if`", self.peek().text()),
                    self.span(),
                ))
            }
            Tok::While => {
                self.bump();
                let cond = self.expr()?;
                let body = self.suite()?;
                StmtKind::While { cond, body }
            }
            Tok::For => {
                self.bump();
                let var = self.ident("loop variable")?;
                self.expect(&Tok::In, "`in`")?;
                let iter = self.expr()?;
                let body = self.suite()?;
                StmtKind::For { var, iter, body }
            }
            Tok::Break => {
                self.bump();
                self.end_of_line()?;
                StmtKind::Break
            }
            Tok::Continue => {
                self.bump();
                self.end_of_line()?;
                StmtKind::Continue
            }
            Tok::Pass => {
                self.bump();
                self.end_of_line()?;
                StmtKind::Pass
            }
            Tok::Scope => {
                self.bump();
                StmtKind::Scope(self.suite()?)
            }
            Tok::Fn | Tok::Type | Tok::Use | Tok::Const | Tok::Pub | Tok::Test => {
                return Err(Diagnostic::error(
                    "B1012",
                    format!(
                        "`{}` declarations are only allowed at the top level",
                        self.peek().text()
                    ),
                    self.span(),
                ))
            }
            Tok::Indent => return Err(self.unexpected("a statement")),
            _ => {
                let target = self.expr()?;
                let op = match self.peek() {
                    Tok::Assign => Some(AssignOp::Set),
                    Tok::PlusAssign => Some(AssignOp::Add),
                    Tok::MinusAssign => Some(AssignOp::Sub),
                    Tok::StarAssign => Some(AssignOp::Mul),
                    Tok::SlashAssign => Some(AssignOp::Div),
                    _ => None,
                };
                if let Some(op) = op {
                    if !target.is_place() {
                        return Err(Diagnostic::error(
                            "B1014",
                            "invalid assignment target",
                            target.span,
                        )
                        .with_note("only variables, record fields and list/buffer elements can be assigned"));
                    }
                    self.bump();
                    let value = self.expr()?;
                    self.end_of_line()?;
                    StmtKind::Assign { target, op, value }
                } else {
                    self.end_of_line()?;
                    StmtKind::Expr(target)
                }
            }
        };
        Ok(Stmt {
            kind,
            span: start.to(self.prev_span()),
        })
    }

    /// After `if`/`elif` has been consumed.
    fn if_rest(&mut self, start: Span) -> PResult<Stmt> {
        let cond = self.expr()?;
        let then = self.suite()?;
        let els = if self.at(&Tok::Elif) {
            let s = self.span();
            self.bump();
            let nested = self.if_rest(s)?;
            Some(Block {
                span: nested.span,
                stmts: vec![nested],
            })
        } else if self.at(&Tok::Else) {
            self.bump();
            Some(self.suite()?)
        } else {
            None
        };
        Ok(Stmt {
            kind: StmtKind::If { cond, then, els },
            span: start.to(self.prev_span()),
        })
    }

    // ----- expressions -------------------------------------------------

    pub fn expr(&mut self) -> PResult<Expr> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.and_expr()?;
        while self.eat(&Tok::Or) {
            let rhs = self.and_expr()?;
            lhs = bin(BinOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }

    fn and_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.cmp_expr()?;
        while self.eat(&Tok::And) {
            let rhs = self.cmp_expr()?;
            lhs = bin(BinOp::And, lhs, rhs);
        }
        Ok(lhs)
    }

    fn cmp_op(&self) -> Option<BinOp> {
        Some(match self.peek() {
            Tok::EqEq => BinOp::Eq,
            Tok::NotEq => BinOp::Ne,
            Tok::Lt => BinOp::Lt,
            Tok::Le => BinOp::Le,
            Tok::Gt => BinOp::Gt,
            Tok::Ge => BinOp::Ge,
            _ => return None,
        })
    }

    fn cmp_expr(&mut self) -> PResult<Expr> {
        let lhs = self.additive()?;
        if let Some(op) = self.cmp_op() {
            self.bump();
            let rhs = self.additive()?;
            if self.cmp_op().is_some() {
                return Err(Diagnostic::error(
                    "B1013",
                    "chained comparisons are not allowed",
                    self.span(),
                )
                .with_help("write `a < b and b < c`"));
            }
            return Ok(bin(op, lhs, rhs));
        }
        Ok(lhs)
    }

    fn additive(&mut self) -> PResult<Expr> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.multiplicative()?;
            lhs = bin(op, lhs, rhs);
        }
    }

    fn multiplicative(&mut self) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Rem,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.unary()?;
            lhs = bin(op, lhs, rhs);
        }
    }

    fn unary(&mut self) -> PResult<Expr> {
        let start = self.span();
        let tok = self.peek().clone();
        match tok {
            Tok::Not | Tok::Minus => {
                self.bump();
                let e = self.unary()?;
                let op = if tok == Tok::Not {
                    UnOp::Not
                } else {
                    UnOp::Neg
                };
                Ok(Expr {
                    span: start.to(e.span),
                    kind: ExprKind::Unary(op, Box::new(e)),
                })
            }
            Tok::View | Tok::Edit | Tok::Take => {
                self.bump();
                let e = self.unary()?;
                if !e.is_place() {
                    return Err(Diagnostic::error(
                        "B1015",
                        format!(
                            "`{}` needs a variable, field or element, not an expression",
                            tok.text()
                        ),
                        e.span,
                    )
                    .with_note("references and ownership transfers apply to named storage only"));
                }
                let span = start.to(e.span);
                let kind = match tok {
                    Tok::View => ExprKind::Borrow(BorrowKind::View, Box::new(e)),
                    Tok::Edit => ExprKind::Borrow(BorrowKind::Edit, Box::new(e)),
                    _ => ExprKind::Take(Box::new(e)),
                };
                Ok(Expr { kind, span })
            }
            Tok::Try => {
                self.bump();
                let e = self.unary()?;
                Ok(Expr {
                    span: start.to(e.span),
                    kind: ExprKind::Try(Box::new(e)),
                })
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.bump();
                    let name = self.ident("field name")?;
                    e = Expr {
                        span: e.span.to(name.span),
                        kind: ExprKind::Field(Box::new(e), name),
                    };
                }
                Tok::LBracket => {
                    self.bump();
                    let idx = self.expr()?;
                    let end = self.expect(&Tok::RBracket, "`]`")?;
                    e = Expr {
                        span: e.span.to(end),
                        kind: ExprKind::Index(Box::new(e), Box::new(idx)),
                    };
                }
                Tok::LParen => {
                    self.bump();
                    let mut args = Vec::new();
                    if !self.at(&Tok::RParen) {
                        loop {
                            let name = if matches!(self.peek(), Tok::Ident(_))
                                && self.peek_at(1) == &Tok::Assign
                            {
                                let n = self.ident("argument name")?;
                                self.bump();
                                Some(n)
                            } else {
                                None
                            };
                            let value = self.expr()?;
                            args.push(Arg { name, value });
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                    }
                    let end = self.expect(&Tok::RParen, "`)` or `,`")?;
                    e = Expr {
                        span: e.span.to(end),
                        kind: ExprKind::Call(Box::new(e), args),
                    };
                }
                _ => return Ok(e),
            }
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(v) => ExprKind::Int(v),
            Tok::Float(v) => ExprKind::Float(v),
            Tok::Str(s) => ExprKind::Str(s),
            Tok::True => ExprKind::Bool(true),
            Tok::False => ExprKind::Bool(false),
            Tok::Ident(name) => {
                if let Some((_, msg)) = UNSUPPORTED.iter().find(|(w, _)| *w == name) {
                    if matches!(self.peek_at(1), Tok::Ident(_)) {
                        return Err(Diagnostic::error("B1017", msg.to_string(), span));
                    }
                }
                ExprKind::Name(name)
            }
            Tok::LParen => {
                self.bump();
                let inner = self.expr()?;
                let end = self.expect(&Tok::RParen, "`)`")?;
                return Ok(Expr {
                    kind: inner.kind,
                    span: span.to(end),
                });
            }
            Tok::LBracket => {
                self.bump();
                let mut elems = Vec::new();
                if !self.at(&Tok::RBracket) {
                    loop {
                        elems.push(self.expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                        if self.at(&Tok::RBracket) {
                            break;
                        }
                    }
                }
                let end = self.expect(&Tok::RBracket, "`]` or `,`")?;
                return Ok(Expr {
                    kind: ExprKind::List(elems),
                    span: span.to(end),
                });
            }
            _ => return Err(self.unexpected("an expression")),
        };
        self.bump();
        Ok(Expr { kind, span })
    }
}

fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr {
        span: lhs.span.to(rhs.span),
        kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use beaver_source::SourceMap;
    use std::path::PathBuf;

    fn parse_src(src: &str) -> Result<Module, Vec<Diagnostic>> {
        let mut sm = SourceMap::new();
        let id = sm.add("t.bv", PathBuf::new(), src);
        let toks = lex(id, sm.file(id))?;
        parse(&toks)
    }

    fn first_fn_stmts(src: &str) -> Vec<Stmt> {
        match &parse_src(src).unwrap().items[0] {
            Item::Fn(f) => f.body.stmts.clone(),
            _ => panic!(),
        }
    }

    fn expr_of(src: &str) -> Expr {
        let stmts = first_fn_stmts(&format!("fn f():\n    {src}\n"));
        match &stmts[0].kind {
            StmtKind::Expr(e) => e.clone(),
            other => panic!("{other:?}"),
        }
    }

    fn shape(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(v) => v.to_string(),
            ExprKind::Name(n) => n.clone(),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Binary(op, l, r) => format!("({} {} {})", shape(l), op.symbol(), shape(r)),
            ExprKind::Unary(UnOp::Neg, e) => format!("(-{})", shape(e)),
            ExprKind::Unary(UnOp::Not, e) => format!("(not {})", shape(e)),
            ExprKind::Call(c, args) => format!(
                "{}({})",
                shape(c),
                args.iter()
                    .map(|a| shape(&a.value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ExprKind::Field(b, f) => format!("{}.{}", shape(b), f.name),
            ExprKind::Index(b, i) => format!("{}[{}]", shape(b), shape(i)),
            ExprKind::Try(e) => format!("(try {})", shape(e)),
            ExprKind::Take(e) => format!("(take {})", shape(e)),
            ExprKind::Borrow(k, e) => format!("({:?} {})", k, shape(e)),
            other => format!("{other:?}"),
        }
    }

    fn err_code(src: &str) -> &'static str {
        parse_src(src).unwrap_err()[0].code
    }

    #[test]
    fn precedence() {
        assert_eq!(shape(&expr_of("a + b * c - d")), "((a + (b * c)) - d)");
        assert_eq!(shape(&expr_of("a or b and c")), "(a or (b and c))");
        assert_eq!(shape(&expr_of("a < b + 1 and c")), "((a < (b + 1)) and c)");
        assert_eq!(shape(&expr_of("-a * b")), "((-a) * b)");
        assert_eq!(shape(&expr_of("not a == b")), "((not a) == b)");
        assert_eq!(shape(&expr_of("(a + b) * c")), "((a + b) * c)");
    }

    #[test]
    fn postfix_chains() {
        assert_eq!(shape(&expr_of("m.f(x)[0].y")), "m.f(x)[0].y");
        assert_eq!(shape(&expr_of("try divide(1, 2)")), "(try divide(1, 2))");
        assert_eq!(
            shape(&expr_of("f(edit p.x, take q)")),
            "f((Edit p.x), (take q))"
        );
    }

    #[test]
    fn chained_comparison_rejected() {
        assert_eq!(err_code("fn f():\n    a < b < c\n"), "B1013");
    }

    #[test]
    fn borrow_of_expression_rejected() {
        assert_eq!(err_code("fn f():\n    let a = view (1 + 2)\n"), "B1015");
        assert_eq!(err_code("fn f():\n    g(take h())\n"), "B1015");
    }

    #[test]
    fn invalid_assignment_target() {
        assert_eq!(err_code("fn f():\n    g() = 1\n"), "B1014");
    }

    #[test]
    fn missing_block() {
        assert_eq!(err_code("fn f():\npass\n"), "B1006");
        assert_eq!(err_code("fn f(): pass\n"), "B1006");
    }

    #[test]
    fn unexpected_indent() {
        assert_eq!(err_code("fn f():\n    pass\n        pass\n"), "B1005");
    }

    #[test]
    fn named_args_and_records() {
        let m = parse_src(
            "type P:\n    x: Int\n    y: List[Int]\n\nfn f():\n    let p = P(x=1, y=[])\n",
        )
        .unwrap();
        assert_eq!(m.items.len(), 2);
        match &m.items[0] {
            Item::Type(t) => {
                assert_eq!(t.fields.len(), 2);
                assert_eq!(t.fields[1].ty.arg.as_ref().unwrap().path[0].name, "Int");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn elif_chain_nests() {
        let s = first_fn_stmts("fn f():\n    if a:\n        pass\n    elif b:\n        pass\n    else:\n        pass\n");
        match &s[0].kind {
            StmtKind::If { els: Some(b), .. } => match &b.stmts[0].kind {
                StmtKind::If { els: Some(_), .. } => {}
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn signatures() {
        let m = parse_src(
            "pub fn f(a: Int, b: edit Point, c: take List[String]) -> Result[Void]:\n    pass\n",
        )
        .unwrap();
        match &m.items[0] {
            Item::Fn(f) => {
                assert!(f.public);
                assert_eq!(f.params[1].mode, ParamMode::Edit);
                assert_eq!(f.params[2].mode, ParamMode::Take);
                assert_eq!(f.ret.as_ref().unwrap().path[0].name, "Result");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn tests_and_uses() {
        let m = parse_src("use net.http\n\ntest \"adds\":\n    assert(true)\n").unwrap();
        assert!(matches!(&m.items[0], Item::Use(u) if u.path.len() == 2));
        assert!(matches!(&m.items[1], Item::Test(t) if t.name == "adds"));
    }

    #[test]
    fn unsupported_features_have_specific_errors() {
        assert_eq!(err_code("class Foo:\n    pass\n"), "B1017");
        assert_eq!(err_code("fn f():\n    let a = share b\n"), "B1017");
    }

    #[test]
    fn multiple_errors_recovered() {
        let errs = parse_src("fn f(:\n    pass\n\nfn g(:\n    pass\n").unwrap_err();
        assert_eq!(errs.len(), 2);
    }

    #[test]
    fn top_level_statement_rejected() {
        assert_eq!(err_code("let x = 1\n"), "B1012");
    }
}
