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
    assert!(err("if true { print(1) }").contains("if is not compiled yet"));
    assert!(err("let xs = [1]").contains("a list is not compiled yet"));
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

#[test]
fn globals_hold_values() {
    assert_eq!(
        out("let x = 3
print(x * 2)"),
        ["6"]
    );
    assert_eq!(
        out("let x = 1
x = x + 41
print(x)"),
        ["42"]
    );
    // Assignment is an expression, so it has the value it assigned.
    assert_eq!(
        out("let a = 1
let b = 2
print(a = b = 7)
print(a + b)"),
        ["7", "14"]
    );
    assert_eq!(
        out(r#"let s = "ab"
s = s + "c"
print(s)"#),
        ["abc"]
    );
}

#[test]
fn undefined_names_are_errors() {
    assert!(err("print(nope)").contains("nope is not defined"));
    // Assigning must not quietly create a global; that turns every typo into a new variable.
    let e = err("let total = 0
totl = 5");
    assert!(e.contains("totl is not defined"), "{e}");
    assert!(e.contains("line 2"), "{e}");
}

#[test]
fn locals_live_in_blocks() {
    assert_eq!(
        out("{ let x = 5
print(x) }"),
        ["5"]
    );
    assert_eq!(
        out("{ let a = 1
let b = 2
print(a + b) }"),
        ["3"]
    );
    assert_eq!(
        out("{ let x = 1
x = x + 1
print(x) }"),
        ["2"]
    );
    // Leaving the block ends the local; the name then means the global again.
    assert!(
        err("{ let only = 1 }
print(only)")
        .contains("only is not defined")
    );
}

#[test]
fn inner_names_shadow_outer_ones() {
    let src = "let x = \"global\"
{
  let x = \"outer\"
  {
    let x = \"inner\"
    print(x)
  }
  print(x)
}
print(x)";
    assert_eq!(out(src), ["inner", "outer", "global"]);
    // The initialiser runs before the new name exists, so it sees the outer one.
    assert_eq!(
        out("{ let x = 2
{ let x = x * 10
print(x) } }"),
        ["20"]
    );
}

#[test]
fn a_block_cannot_declare_a_name_twice() {
    assert!(
        err("{ let x = 1
let x = 2 }")
        .contains("x is already declared in this block")
    );
    // At the top level it is a global, which can be redefined.
    assert_eq!(
        out("let x = 1
let x = 2
print(x)"),
        ["2"]
    );
}

#[test]
fn locals_are_stack_slots_not_names() {
    let chunk = compile(
        &parse(
            "{ let a = 1
let b = 2
print(b) }",
        )
        .unwrap(),
    )
    .unwrap();
    let text = chunk.disassemble();
    assert!(text.contains("GETLOCAL        1"), "{text}");
    // Two locals, two pops when the block ends, and no global lookups at all.
    assert_eq!(text.matches("POP").count(), 2, "{text}");
    assert!(!text.contains("GLOBAL"), "{text}");
}
