use ember::vm::Vm;
use ember::{needs_more, repl_line};

fn line(vm: &mut Vm, src: &str) -> Option<String> {
    repl_line(vm, src).unwrap_or_else(|e| panic!("{src}: {e}"))
}

#[test]
fn what_one_line_defines_the_next_can_use() {
    let mut vm = Vm::new();
    assert_eq!(line(&mut vm, "let x = 2"), None);
    assert_eq!(line(&mut vm, "fn double(n) { return n * 2 }"), None);
    assert_eq!(line(&mut vm, "double(x) + 17"), Some("21".into()));
    // Closures made on one line keep working on later ones.
    line(
        &mut vm,
        "fn counter() { let n = 0\n return fn() { n = n + 1\n return n } }",
    );
    line(&mut vm, "let c = counter()");
    line(&mut vm, "c()");
    assert_eq!(line(&mut vm, "c()"), Some("2".into()));
}

#[test]
fn a_bare_expression_is_shown_the_way_it_would_be_written() {
    let mut vm = Vm::new();
    assert_eq!(line(&mut vm, "\"1\""), Some("\"1\"".into()));
    assert_eq!(line(&mut vm, "1"), Some("1".into()));
    assert_eq!(
        line(&mut vm, "[1, \"a\", {\"k\": true}]"),
        Some(r#"[1, "a", {"k": true}]"#.into())
    );
    // nil is not echoed, and print's own output goes where print's output always goes.
    assert_eq!(line(&mut vm, "nil"), None);
    assert_eq!(line(&mut vm, "print(\"hi\")"), None);
    assert_eq!(vm.output, ["hi"]);
    // Only a closing bare expression is shown; one earlier on the line is dropped as usual.
    assert_eq!(line(&mut vm, "1 + 1\nlet y = 5"), None);
}

#[test]
fn an_error_does_not_end_the_session() {
    let mut vm = Vm::new();
    line(&mut vm, "let total = 10");
    let e = repl_line(&mut vm, "total / 0").unwrap_err();
    assert!(e.to_string().contains("division by zero"), "{e}");
    assert!(repl_line(&mut vm, "let = 1").is_err());
    assert_eq!(line(&mut vm, "total"), Some("10".into()));
}

#[test]
fn an_unfinished_entry_waits_for_more_but_a_mistake_does_not() {
    for unfinished in [
        "if true {",
        "fn f(n) {\n  return n",
        "print(1,",
        "\"an open string",
        "let x = 1 +",
    ] {
        assert!(
            needs_more(unfinished),
            "{unfinished:?} should wait for another line"
        );
    }
    for done_or_wrong in ["1 + 1", "let x = 1", "1 +* 2", ")", "let = 3"] {
        assert!(
            !needs_more(done_or_wrong),
            "{done_or_wrong:?} should not wait"
        );
    }
}
