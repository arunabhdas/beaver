//! Type-checker and ownership-checker unit tests on small programs.

use beaver_sema::{analyze, PackageInput, ProgramInput, ProgramKind};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn check(src: &str) -> Result<beaver_sema::hir::Program, Vec<(String, String)>> {
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
    analyze(&input).map_err(|f| {
        f.diagnostics
            .iter()
            .map(|d| (d.code.to_string(), d.message.clone()))
            .collect()
    })
}

fn ok(src: &str) {
    if let Err(e) = check(src) {
        panic!("expected success, got {e:?}\n{src}");
    }
}

fn err(src: &str, code: &str) {
    match check(src) {
        Ok(_) => panic!("expected {code}, but the program was accepted\n{src}"),
        Err(e) => assert!(
            e.iter().any(|(c, _)| c == code),
            "expected {code}, got {e:?}\n{src}"
        ),
    }
}

fn main_body(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {l}\n")).collect();
    format!("fn main():\n{indented}")
}

// ----- type checking -------------------------------------------------------

#[test]
fn local_inference_and_annotations() {
    let p = check(&main_body(
        "let a = 1\nlet b = 2.5\nlet c = \"s\"\nlet d: List[Int] = []\nvar e = true",
    ))
    .unwrap();
    let main = &p.functions[p.entry.unwrap().0];
    let tys: Vec<String> = main
        .locals
        .iter()
        .map(|l| l.ty.display(&p.records))
        .collect();
    assert_eq!(tys, ["Int", "Float", "String", "List[Int]", "Bool"]);
}

#[test]
fn no_implicit_numeric_conversion() {
    err(&main_body("let x = 1 + 2.0"), "B2005");
    err(&main_body("let x: Byte = 3"), "B2003");
    ok(&main_body(
        "let x = Float(1) + 2.0\nlet y = Byte(3)\nlet z = Int(y) + 1",
    ));
}

#[test]
fn conditions_must_be_bool() {
    err(&main_body("if 1:\n    pass"), "B2004");
    err(&main_body("while \"x\":\n    pass"), "B2004");
    err(&main_body("let b = 1 and true"), "B2005");
}

#[test]
fn return_paths() {
    err(
        "fn f(b: Bool) -> Int:\n    if b:\n        return 1\n\nfn main():\n    pass\n",
        "B2006",
    );
    ok("fn f(b: Bool) -> Int:\n    if b:\n        return 1\n    else:\n        return 2\n\nfn main():\n    pass\n");
    ok("fn f() -> Int:\n    while true:\n        return 1\n\nfn main():\n    pass\n");
    err(
        "fn f() -> Int:\n    while true:\n        break\n\nfn main():\n    pass\n",
        "B2006",
    );
    err("fn f():\n    return 1\n\nfn main():\n    pass\n", "B2029");
    err(
        "fn f() -> Int:\n    return\n\nfn main():\n    pass\n",
        "B2029",
    );
}

#[test]
fn records_copy_and_equality_are_structural() {
    let p = check("type P:\n    x: Int\n\ntype N:\n    name: String\n\ntype L:\n    xs: List[Int]\n\nfn main():\n    pass\n").unwrap();
    let by_name = |n: &str| p.records.iter().find(|r| r.name == n).unwrap();
    assert!(by_name("P").is_copy && by_name("P").supports_eq);
    assert!(!by_name("N").is_copy && by_name("N").supports_eq);
    assert!(!by_name("L").is_copy && !by_name("L").supports_eq);
    err("type L:\n    xs: List[Int]\n\nfn main():\n    let a = L(xs=[])\n    let b = L(xs=[])\n    print(a == b)\n", "B2025");
}

#[test]
fn recursive_records() {
    ok("type T:\n    kids: List[T]\n\nfn main():\n    pass\n");
    err(
        "type T:\n    next: Result[T]\n\nfn main():\n    pass\n",
        "B2021",
    );
    err(
        "type A:\n    b: B\n\ntype B:\n    a: A\n\nfn main():\n    pass\n",
        "B2021",
    );
}

#[test]
fn constructors_need_every_field_once_by_name() {
    let rec = "type P:\n    x: Int\n    y: Int\n\n";
    err(&format!("{rec}fn main():\n    let p = P(1, 2)\n"), "B2011");
    err(
        &format!("{rec}fn main():\n    let p = P(x=1, x=2, y=3)\n"),
        "B2011",
    );
    err(
        &format!("{rec}fn main():\n    let p = P(x=1, z=2, y=3)\n"),
        "B2011",
    );
    ok(&format!(
        "{rec}fn main():\n    let p = P(y=1, x=2)\n    print(p.y)\n"
    ));
}

#[test]
fn constants_are_evaluated_and_type_checked() {
    ok("const A: Int = 2 * 3 - 1\nconst B: Bool = A > 4\n\nfn main():\n    print(A)\n");
    err("const A: Int = 1.5\n\nfn main():\n    pass\n", "B2003");
    err(
        "const A: Int = B\nconst B: Int = A\n\nfn main():\n    pass\n",
        "B2023",
    );
    err("const A: Int = 1 / 0\n\nfn main():\n    pass\n", "B2023");
    err("const L: List[Int] = []\n\nfn main():\n    pass\n", "B2023");
}

#[test]
fn result_typing() {
    ok("fn f() -> Result[Int]:\n    return fail(\"x\")\n\nfn g() -> Result[Void]:\n    let v = try f()\n    print(v)\n    return ok()\n\nfn main():\n    pass\n");
    err(
        "fn f() -> Result[Int]:\n    return ok(\"s\")\n\nfn main():\n    pass\n",
        "B2003",
    );
    err(
        "fn f() -> Result[Int]:\n    let x = try 3\n    return ok(x)\n\nfn main():\n    pass\n",
        "B2008",
    );
    err(
        &main_body("let r = ok()\nlet x = unwrap(take r)\nprint(x)"),
        "B2019",
    );
}

#[test]
fn builtins_are_typed() {
    err(&main_body("print([1])"), "B2005");
    err(&main_body("print(1, 2)"), "B2010");
    err(&main_body("let n = len(5)"), "B2005");
    err(
        &main_body("var b = Buffer(size=2)\nappend(edit b, Byte(1))"),
        "B2005",
    );
    err(&main_body("let b = Buffer(2)"), "B2010");
    err(&main_body("let x = Byte(1.5)"), "B2005");
    ok(&main_body(
        "let xs = [1]\nlet n = len(xs) + len(\"ab\")\nassert_eq(n, 3)",
    ));
}

#[test]
fn names_scopes_and_shadowing() {
    err(&main_body("let a = 1\nlet a = 2"), "B2002");
    ok(&main_body(
        "let a = 1\nscope:\n    let a = \"inner\"\n    print(a)\nprint(a)",
    ));
    err(&main_body("scope:\n    let a = 1\nprint(a)"), "B2001");
    err(
        "fn f(a: Int, a: Int):\n    pass\n\nfn main():\n    pass\n",
        "B2002",
    );
    err(
        "fn f(a: Int):\n    let a = 2\n\nfn main():\n    pass\n",
        "B2002",
    );
    err(
        "fn f():\n    pass\n\nfn f():\n    pass\n\nfn main():\n    pass\n",
        "B2002",
    );
}

#[test]
fn indexing_rules() {
    err(&main_body("let xs = [1]\nprint(xs[true])"), "B2016");
    err(&main_body("let s = \"ab\"\nprint(s[0])"), "B2017");
    err(&main_body("let n = 3\nprint(n[0])"), "B2016");
}

#[test]
fn entry_point_rules() {
    err("pub fn main():\n    pass\n", "B2020");
    err("fn main(x: Int):\n    pass\n", "B2020");
    err("fn other():\n    pass\n", "B2020");
}

#[test]
fn void_restrictions() {
    err("type R:\n    v: Void\n\nfn main():\n    pass\n", "B2019");
    err(
        "fn f(v: Void):\n    pass\n\nfn main():\n    pass\n",
        "B2019",
    );
    err(&main_body("let xs: List[Void] = []"), "B2019");
}

// ----- ownership and borrowing ---------------------------------------------

#[test]
fn shared_views_may_overlap() {
    ok(&main_body(
        "var n = 1\nlet a = view n\nlet b = view n\nprint(a + b)",
    ));
}

#[test]
fn edit_conflicts_with_any_active_borrow() {
    err(
        &main_body("var n = 1\nlet a = view n\nlet b = edit n"),
        "B3002",
    );
    err(
        &main_body("var n = 1\nlet a = edit n\nlet b = view n"),
        "B3002",
    );
    err(
        &main_body("var n = 1\nlet a = edit n\nlet b = edit n"),
        "B3002",
    );
}

#[test]
fn scope_ends_named_borrows() {
    ok(&main_body(
        "var n = 1\nscope:\n    let a = view n\n    print(a)\nlet b = edit n\nb = 3",
    ));
}

#[test]
fn field_borrows_lock_the_whole_owner() {
    err("type P:\n    a: Int\n    b: Int\n\nfn main():\n    var p = P(a=1, b=2)\n    let x = view p.a\n    let y = edit p.b\n", "B3002");
}

#[test]
fn call_borrows_end_at_statement_end() {
    ok("fn bump(n: edit Int):\n    n += 1\n\nfn main():\n    var n = 1\n    bump(edit n)\n    bump(edit n)\n    print(n)\n");
    err("fn two(a: edit Int, b: edit Int):\n    pass\n\nfn main():\n    var n = 1\n    two(edit n, edit n)\n", "B3002");
}

#[test]
fn moves_are_tracked() {
    err(
        &main_body("let s = \"a\"\nlet t = take s\nprint(s)"),
        "B3001",
    );
    err(
        &main_body("let s = \"a\"\nlet t = take s\nlet u = take s"),
        "B3001",
    );
    ok(&main_body("let s = \"a\"\nlet t = take s\nprint(t)"));
    ok(&main_body("let a = 1\nlet b = a\nprint(a + b)"));
}

#[test]
fn implicit_moves_need_take_or_clone() {
    err(&main_body("let s = \"a\"\nlet t = s"), "B3007");
    ok(&main_body("let s = \"a\"\nlet t = clone(s)\nprint(s + t)"));
    err(&main_body("let s = \"a\"\nlet xs = [s]"), "B3007");
    err(
        "fn f() -> String:\n    let s = \"a\"\n    return s\n\nfn main():\n    pass\n",
        "B3007",
    );
    ok("fn f() -> String:\n    let s = \"a\"\n    return take s\n\nfn main():\n    pass\n");
}

#[test]
fn joins_require_consistent_ownership_when_used_later() {
    err(
        &main_body("let s = \"a\"\nif true:\n    let t = take s\nprint(s)"),
        "B3009",
    );
    ok(&main_body(
        "let s = \"a\"\nif true:\n    let t = take s\nprint(1)",
    ));
    ok(&main_body(
        "let s = \"a\"\nif true:\n    let t = take s\nelse:\n    let u = take s\nprint(1)",
    ));
    ok(&main_body(
        "let s = \"a\"\nif true:\n    let t = take s\n    return\nprint(s)",
    ));
}

#[test]
fn loops_may_not_move_outer_values_on_back_edges() {
    err(
        &main_body("let s = \"a\"\nwhile true:\n    let t = take s"),
        "B3010",
    );
    err(&main_body("let s = \"a\"\nfor i in range(0, 2):\n    if i == 1:\n        let t = take s\n        continue"), "B3010");
    ok(&main_body("let s = \"a\"\nfor i in range(0, 2):\n    if i == 1:\n        let t = take s\n        break"));
    ok(&main_body(
        "for i in range(0, 2):\n    let s = \"fresh\"\n    let t = take s",
    ));
}

#[test]
fn references_never_escape() {
    err(
        "fn f(s: String) -> String:\n    return s\n\nfn main():\n    pass\n",
        "B3008",
    );
    err(
        &main_body("var n = 1\nlet r = view n\nlet xs = [r]"),
        "B3008",
    );
    err("type B:\n    v: Int\n\nfn main():\n    let n = 1\n    let r = view n\n    let b = B(v=r)\n", "B3008");
    err(
        &main_body("let n = 1\nlet r = view n\nlet x: Result[Int] = ok(r)"),
        "B3008",
    );
    ok(&main_body(
        "let n = 1\nlet r = view n\nlet copy = r + 0\nlet xs = [clone(r)]",
    ));
}

#[test]
fn edit_needs_mutable_storage() {
    err(&main_body("let n = 1\nlet w = edit n"), "B3004");
    err(
        "fn f(xs: List[Int]):\n    append(edit xs, 1)\n\nfn main():\n    pass\n",
        "B3004",
    );
    ok("fn f(xs: edit List[Int]):\n    append(edit xs, 1)\n\nfn main():\n    var xs = [1]\n    f(edit xs)\n");
    err(&main_body("var n = 1\nlet r = view n\nr = 2"), "B3011");
}

#[test]
fn iteration_holds_a_read_borrow() {
    err(
        &main_body("var xs = [1]\nfor x in xs:\n    xs[0] = 2"),
        "B3014",
    );
    err(
        &main_body("var xs = [1]\nfor x in xs:\n    append(edit xs, 2)"),
        "B3002",
    );
    ok(&main_body(
        "var xs = [1]\nfor x in xs:\n    print(x + len(xs))\nappend(edit xs, 2)",
    ));
}

#[test]
fn partial_moves_are_rejected() {
    err(
        "type P:\n    s: String\n\nfn main():\n    let p = P(s=\"a\")\n    let t = take p.s\n",
        "B3006",
    );
    err(&main_body("let xs = [\"a\"]\nlet t = take xs[0]"), "B3006");
    err(&main_body("let xs = [\"a\"]\nlet t = xs[0]"), "B3006");
    ok(&main_body(
        "let xs = [\"a\"]\nlet t = clone(xs[0])\nprint(xs[0])",
    ));
}

#[test]
fn conditional_operands_cannot_move() {
    err("fn eat(s: take String) -> Bool:\n    return true\n\nfn main():\n    let s = \"a\"\n    let b = true or eat(take s)\n", "B3015");
}

#[test]
fn argument_modes_must_match_parameters() {
    let f = "fn e(n: edit Int):\n    pass\n\nfn t(s: take String):\n    pass\n\nfn v(n: Int):\n    pass\n\n";
    err(
        &format!("{f}fn main():\n    var n = 1\n    e(n)\n"),
        "B3017",
    );
    err(
        &format!("{f}fn main():\n    var n = 1\n    v(edit n)\n"),
        "B3017",
    );
    err(
        &format!("{f}fn main():\n    let s = \"a\"\n    t(s)\n"),
        "B3007",
    );
    ok(&format!("{f}fn main():\n    var n = 1\n    e(edit n)\n    t(\"temporary\")\n    let s = \"a\"\n    t(take s)\n    v(n)\n"));
}

#[test]
fn reborrowing_through_edit_parameters() {
    ok("fn inner(n: edit Int):\n    n += 1\n\nfn outer(n: edit Int):\n    inner(edit n)\n    n += 1\n\nfn main():\n    var x = 0\n    outer(edit x)\n");
    err("fn inner(n: edit Int):\n    n += 1\n\nfn outer(n: view Int):\n    inner(edit n)\n\nfn main():\n    pass\n", "B3004");
}
