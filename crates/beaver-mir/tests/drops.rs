//! Drop elaboration checks on lowered MIR: every path from entry to a
//! `return` drops each owned value exactly once, and never drops moved ones.

use beaver_mir::*;
use beaver_sema::{analyze, PackageInput, ProgramInput, ProgramKind};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn lower_src(src: &str) -> MirProgram {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/main.bv"), src).unwrap();
    let input = ProgramInput {
        packages: vec![PackageInput {
            name: "t".into(),
            dir: tmp.path().to_path_buf(),
            display_prefix: String::new(),
            entry: PathBuf::from("src/main.bv"),
            deps: BTreeMap::new(),
        }],
        root: 0,
        kind: ProgramKind::Bin,
        test_files: Vec::new(),
    };
    let prog = analyze(&input).unwrap_or_else(|f| panic!("{}", f.render()));
    lower(&prog)
}

fn func<'a>(p: &'a MirProgram, name: &str) -> &'a MirFn {
    p.functions.iter().find(|f| f.name == name).unwrap()
}

fn local_named(f: &MirFn, name: &str) -> Local {
    Local(
        f.locals
            .iter()
            .position(|l| l.name.as_deref() == Some(name))
            .unwrap(),
    )
}

/// Enumerate all acyclic paths entry -> Return and count drops of `l` and
/// whether it was moved (as a whole) on the path.
fn paths_drop_counts(f: &MirFn, l: Local) -> Vec<(usize, bool)> {
    fn walk(
        f: &MirFn,
        b: usize,
        l: Local,
        drops: usize,
        moved: bool,
        seen: &mut Vec<usize>,
        out: &mut Vec<(usize, bool)>,
    ) {
        if seen.contains(&b) {
            return; // ignore back edges; each loop body is checked once
        }
        seen.push(b);
        let mut d = drops;
        let mut m = moved;
        for s in &f.blocks[b].stmts {
            match s {
                Stmt::Drop(p) if p.local == l && p.proj.is_empty() => d += 1,
                Stmt::Assign(_, r) => {
                    if rvalue_moves(r, l) {
                        m = true;
                    }
                }
                Stmt::Append(_, Operand::Move(p)) if p.local == l => m = true,
                _ => {}
            }
        }
        match &f.blocks[b].term {
            Terminator::Return => out.push((d, m)),
            Terminator::Goto(t) => walk(f, t.0, l, d, m, seen, out),
            Terminator::Branch(_, t, e) => {
                walk(f, t.0, l, d, m, &mut seen.clone(), out);
                walk(f, e.0, l, d, m, seen, out);
            }
            Terminator::Unreachable => {}
        }
        seen.pop();
    }
    let mut out = Vec::new();
    walk(f, 0, l, 0, false, &mut Vec::new(), &mut out);
    out
}

fn rvalue_moves(r: &Rvalue, l: Local) -> bool {
    let mv = |o: &Operand| matches!(o, Operand::Move(p) if p.local == l && p.proj.is_empty());
    match r {
        Rvalue::Use(o) | Rvalue::Unwrap(o, _) | Rvalue::UnwrapError(o, _) => mv(o),
        Rvalue::Call(_, ops) | Rvalue::Construct(_, ops) | Rvalue::ListLit(_, ops) => {
            ops.iter().any(mv)
        }
        Rvalue::ResultOk(_, Some(o)) | Rvalue::ResultFail(_, o) => mv(o),
        _ => false,
    }
}

fn assert_exactly_once(f: &MirFn, l: Local) {
    let paths = paths_drop_counts(f, l);
    assert!(!paths.is_empty(), "no return paths");
    for (drops, moved) in paths {
        let total = drops + usize::from(moved);
        assert_eq!(
            total, 1,
            "local _{} released {drops} time(s), moved={moved} on a path\n{f}",
            l.0
        );
    }
}

#[test]
fn early_returns_drop_every_live_owner() {
    let p = lower_src(
        "fn f(n: Int) -> Int:\n    let a = \"a\" + \"b\"\n    if n == 0:\n        return 1\n    let b = [\"x\"]\n    if n == 1:\n        return 2\n    return 3\n\nfn main():\n    print(f(0))\n",
    );
    let f = func(&p, "f");
    assert_exactly_once(f, local_named(f, "a"));
    let b = local_named(f, "b");
    for (drops, _) in paths_drop_counts(f, b) {
        assert!(drops <= 1);
    }
}

#[test]
fn moved_values_are_not_dropped() {
    let p = lower_src("fn eat(s: take String):\n    print(s)\n\nfn main():\n    let s = \"x\" + \"y\"\n    eat(take s)\n");
    let f = func(&p, "main");
    let s = local_named(f, "s");
    for (drops, _) in paths_drop_counts(f, s) {
        assert_eq!(drops, 0, "{f}");
    }
    // The callee owns and drops its `take` parameter.
    let eat = func(&p, "eat");
    assert_exactly_once(eat, local_named(eat, "s"));
}

#[test]
fn branch_only_moves_get_a_fixup_drop_on_the_other_path() {
    let p = lower_src(
        "fn eat(s: take String):\n    print(s)\n\nfn main():\n    let s = \"x\" + \"y\"\n    if len(s) > 1:\n        eat(take s)\n    else:\n        print(\"no\")\n    print(1)\n",
    );
    let f = func(&p, "main");
    let s = local_named(f, "s");
    let paths = paths_drop_counts(f, s);
    assert_eq!(paths.len(), 2);
    // On the `take` path the value went through a temporary into `eat`; on
    // the other path it is dropped exactly once.
    assert!(paths.iter().any(|(d, _)| *d == 1), "{f}");
    assert!(paths.iter().all(|(d, _)| *d <= 1), "{f}");
}

#[test]
fn loop_exits_release_body_owners() {
    let p = lower_src(
        "fn main():\n    var i = 0\n    while i < 3:\n        i += 1\n        let tmp = \"t\" + \"u\"\n        if i == 1:\n            continue\n        if i == 2:\n            break\n        print(tmp)\n",
    );
    let f = func(&p, "main");
    let tmp = local_named(f, "tmp");
    // Each continue/break/fallthrough block drops `tmp` before leaving.
    let drops = f
        .blocks
        .iter()
        .flat_map(|b| &b.stmts)
        .filter(|s| matches!(s, Stmt::Drop(pl) if pl.local == tmp))
        .count();
    assert_eq!(drops, 3, "{f}");
}

#[test]
fn statement_temporaries_are_dropped() {
    let p = lower_src("fn main():\n    print(\"a\" + \"b\")\n");
    let f = func(&p, "main");
    let text = f.to_string();
    assert!(text.contains("concat("), "{text}");
    let concat_dest = f
        .blocks
        .iter()
        .flat_map(|b| &b.stmts)
        .find_map(|s| match s {
            Stmt::Assign(pl, Rvalue::Concat(..)) => Some(pl.local),
            _ => None,
        })
        .unwrap();
    assert_exactly_once(f, concat_dest);
}

#[test]
fn try_error_path_drops_locals_and_moves_the_error() {
    let p = lower_src(
        "fn inner() -> Result[Int]:\n    return fail(\"e\")\n\nfn outer() -> Result[Int]:\n    let keep = \"k\" + \"k\"\n    let n = try inner()\n    print(keep)\n    return ok(n)\n\nfn main():\n    let r = outer()\n    print(is_ok(r))\n",
    );
    let f = func(&p, "outer");
    assert_exactly_once(f, local_named(f, "keep"));
    let text = f.to_string();
    assert!(text.contains("take_err("), "{text}");
    assert!(text.contains("take_ok("), "{text}");
}

#[test]
fn evaluation_order_is_explicit_in_mir() {
    let p = lower_src(
        "fn a() -> Int:\n    return 1\n\nfn b() -> Int:\n    return 2\n\nfn add(x: Int, y: Int) -> Int:\n    return x + y\n\nfn main():\n    print(add(a(), b()))\n",
    );
    let f = func(&p, "main");
    let calls: Vec<usize> = f
        .blocks
        .iter()
        .flat_map(|b| &b.stmts)
        .filter_map(|s| match s {
            Stmt::Assign(_, Rvalue::Call(id, _)) => Some(*id),
            _ => None,
        })
        .collect();
    let id = |n: &str| p.functions.iter().position(|f| f.name == n).unwrap();
    assert_eq!(calls, vec![id("a"), id("b"), id("add")]);
}

#[test]
fn short_circuit_lowers_to_branches() {
    let p = lower_src(
        "fn t() -> Bool:\n    return true\n\nfn main():\n    let x = false and t()\n    print(x)\n",
    );
    let f = func(&p, "main");
    let branches = f
        .blocks
        .iter()
        .filter(|b| matches!(b.term, Terminator::Branch(..)))
        .count();
    assert!(branches >= 1, "{f}");
}

#[test]
fn borrows_are_explicit_and_end() {
    let p = lower_src("fn main():\n    var n = 1\n    scope:\n        let r = view n\n        print(r)\n    n = 2\n");
    let text = func(&p, "main").to_string();
    assert!(text.contains(" = &_"), "{text}");
    assert!(text.contains("end_borrow"), "{text}");
}
