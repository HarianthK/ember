use ember::compiler::compile;
use ember::parser::parse;
use ember::run;

fn out(src: &str) -> Vec<String> {
    run(src).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn err(src: &str) -> String {
    match run(src) {
        Ok(output) => panic!("{src} should have failed, printed {output:?}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn arithmetic_follows_precedence() {
    assert_eq!(out("print(1 + 2 * 3)"), ["7"]);
    assert_eq!(out("print((1 + 2) * 3)"), ["9"]);
    // Left associative at runtime too: the operands must be popped in the right order.
    assert_eq!(out("print(10 - 3 - 2)"), ["5"]);
    assert_eq!(out("print(20 / 4 / 5)"), ["1"]);
    assert_eq!(out("print(7 % 3)"), ["1"]);
    assert_eq!(out("print(-2 * -3)"), ["6"]);
    assert_eq!(out("print(0.1 + 0.2)"), ["0.30000000000000004"]);
}

#[test]
fn comparisons_and_equality() {
    assert_eq!(out("print(1 < 2)"), ["true"]);
    assert_eq!(out("print(2 <= 2)"), ["true"]);
    assert_eq!(out("print(3 > 4)"), ["false"]);
    assert_eq!(out("print(1 == 1)"), ["true"]);
    assert_eq!(out(r#"print("a" == "a")"#), ["true"]);
    // Different types are simply unequal, never an error.
    assert_eq!(out(r#"print(1 == "1")"#), ["false"]);
    assert_eq!(out("print(nil == false)"), ["false"]);
    assert_eq!(out("print(nil != nil)"), ["false"]);
}

#[test]
fn not_uses_truthiness() {
    assert_eq!(out("print(not nil)"), ["true"]);
    assert_eq!(out("print(not 0)"), ["false"]);
    assert_eq!(out(r#"print(not "")"#), ["false"]);
    assert_eq!(out("print(not not true)"), ["true"]);
}

#[test]
fn strings_concatenate() {
    assert_eq!(out(r#"print("ab" + "cd")"#), ["abcd"]);
}

#[test]
fn statements_run_in_order_and_leave_the_stack_clean() {
    assert_eq!(out("print(1)\nprint(2)\n3 + 4\nprint(5)"), ["1", "2", "5"]);
}

#[test]
fn runtime_errors_say_what_and_where() {
    let e = err("print(1)\nprint(1 + \"a\")");
    assert!(e.contains("+ needs two numbers or two strings"), "{e}");
    assert!(e.contains("a number and a string"), "{e}");
    assert!(e.contains("line 2"), "{e}");

    assert!(err("print(-\"a\")").contains("- needs a number, not a string"));
    assert!(err("print(1 / 0)").contains("division by zero"));
    assert!(err("print(1 < nil)").contains("< needs two numbers"));
}

#[test]
fn unfinished_parts_say_so_rather_than_misbehave() {
    assert!(err("let x = 1").contains("let is not compiled yet"));
    assert!(err("print(x)").contains("a variable is not compiled yet"));
    assert!(err("print(1, 2)").contains("print takes one value"));
}

#[test]
fn bytecode_is_what_you_would_write_by_hand() {
    let chunk = compile(&parse("print(1 + 2 * 3)").unwrap()).unwrap();
    let ops: Vec<String> = chunk
        .disassemble()
        .lines()
        .map(|l| l.split_whitespace().nth(2).unwrap().to_string())
        .collect();
    assert_eq!(
        ops,
        [
            "CONSTANT", "CONSTANT", "CONSTANT", "MUL", "ADD", "PRINT", "RETURN"
        ]
    );
}
