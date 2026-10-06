use ember::compiler::compile;
use ember::parser::parse;
use ember::run;
use ember::vm::Vm;

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
    assert!(
        e.contains("+ needs two numbers, two strings or two lists"),
        "{e}"
    );
    assert!(e.contains("a number and a string"), "{e}");
    assert!(e.contains("line 2"), "{e}");

    assert!(err("print(-\"a\")").contains("- needs a number, not a string"));
    assert!(err("print(1 / 0)").contains("division by zero"));
    assert!(err("print(1 < nil)").contains("< needs two numbers"));
}

#[test]
fn bytecode_is_what_you_would_write_by_hand() {
    let script = compile(&parse("1 + 2 * 3").unwrap()).unwrap();
    let ops: Vec<String> = script
        .chunk
        .disassemble()
        .lines()
        .map(|l| l.split_whitespace().nth(2).unwrap().to_string())
        .collect();
    assert_eq!(
        ops,
        [
            "CONSTANT", "CONSTANT", "CONSTANT", "MUL", "ADD", "POP", "NIL", "RETURN"
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
fn a_typo_suggests_the_name_it_was_probably_meant_to_be() {
    let e = err("let total = 0\ntotl = 5");
    assert!(
        e.contains("totl is not defined; did you mean total?"),
        "{e}"
    );
    // Natives and the prelude are globals too; a swapped pair of letters is two edits.
    assert!(err("prnt(1)").contains("did you mean print?"));
    assert!(err("fliter([1], str)").contains("did you mean filter?"));
    // Nothing close, nothing offered, and assignment falls back to its own hint.
    assert!(!err("print(zzz)").contains("did you mean"));
    assert!(err("nope = 1").contains("nope is not defined; declare it with let first"));
    // Short names get fewer edits: nope is two from pop, which is not what anyone meant.
    assert!(!err("print(nope)").contains("did you mean"));
    assert!(err("let ab = 1\nprint(ac)").contains("did you mean ab?"));
    assert!(!err("let ab = 1\nprint(xy)").contains("did you mean"));
    // Two equally close names: the first alphabetically, so the message never varies.
    assert!(err("let cat = 1\nlet car = 2\nprint(caz)").contains("did you mean car?"));
    // A global declared further down but not run yet does not exist, so is not offered.
    assert!(!err("print(totl)\nlet total = 1").contains("did you mean"));
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
    let script = compile(
        &parse(
            "{ let a = 1
let b = 2
let c = b }",
        )
        .unwrap(),
    )
    .unwrap();
    let text = script.chunk.disassemble();
    // Slot 0 holds the running function, so the locals are slots 1, 2 and 3.
    assert!(text.contains("GETLOCAL        2"), "{text}");
    // Three locals, three pops when the block ends, and no global lookups at all.
    assert_eq!(text.matches("POP").count(), 3, "{text}");
    assert!(!text.contains("GLOBAL"), "{text}");
}

#[test]
fn if_takes_one_branch() {
    assert_eq!(
        out("if 1 < 2 { print(\"yes\") } else { print(\"no\") }"),
        ["yes"]
    );
    assert_eq!(
        out("if 1 > 2 { print(\"yes\") } else { print(\"no\") }"),
        ["no"]
    );
    assert_eq!(
        out("if false { print(\"never\") }\nprint(\"after\")"),
        ["after"]
    );
    let chain = "let n = 15
if n % 15 == 0 { print(\"fizzbuzz\") } else if n % 3 == 0 { print(\"fizz\") } else { print(n) }";
    assert_eq!(out(chain), ["fizzbuzz"]);
}

#[test]
fn while_loops_until_false() {
    let src = "let i = 0
let total = 0
while i < 10 {
  i = i + 1
  total = total + i
}
print(total)";
    assert_eq!(out(src), ["55"]);
    assert_eq!(out("while false { print(1) }\nprint(2)"), ["2"]);
}

#[test]
fn and_or_short_circuit_and_return_the_deciding_value() {
    // The right side would fail if it ran, so these prove it did not.
    assert_eq!(out("print(false and undefined_name)"), ["false"]);
    assert_eq!(out("print(true or undefined_name)"), ["true"]);
    assert_eq!(out(r#"print(nil or "default")"#), ["default"]);
    assert_eq!(out(r#"print("first" or "second")"#), ["first"]);
    assert_eq!(out("print(1 and 2)"), ["2"]);
    assert_eq!(out("print(nil and 2)"), ["nil"]);
    assert_eq!(out("print(1 < 2 and 2 < 3)"), ["true"]);
}

#[test]
fn fizzbuzz_runs() {
    let src = "let i = 1
while i <= 15 {
  if i % 15 == 0 { print(\"FizzBuzz\") }
  else if i % 3 == 0 { print(\"Fizz\") }
  else if i % 5 == 0 { print(\"Buzz\") }
  else { print(i) }
  i = i + 1
}";
    let got = out(src);
    assert_eq!(got.len(), 15);
    assert_eq!(got[2], "Fizz");
    assert_eq!(got[4], "Buzz");
    assert_eq!(got[14], "FizzBuzz");
    assert_eq!(got[6], "7");
}

// Every branch and loop pops its condition and its locals; if one side forgot,
// a long loop would leave the stack a little deeper on every pass.
#[test]
fn nothing_leaks_onto_the_stack() {
    let src = "let i = 0
while i < 1000 {
  let doubled = i * 2
  if doubled > 10 and i % 2 == 0 { let x = 1 } else { let y = 2 }
  let z = nil or i
  i = i + 1
}";
    let script = compile(&parse(src).unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    assert_eq!(
        vm.stack_depth(),
        0,
        "the stack should be empty after the program"
    );
}

// Names and operators carry their own position. Before they did, an error inherited
// whatever the compiler had seen last, which could be the statement before.
#[test]
fn errors_point_at_the_operator_or_name_that_failed() {
    let e = err("print(1)\nnope + 1");
    assert!(e.contains("nope is not defined at line 2, column 1"), "{e}");
    let e = err("print(1)\n1 + \"a\"");
    assert!(e.contains("at line 2, column 3"), "{e}");
    let e = err("let x = 1\nlet y = 2\nlet z = -\"a\"");
    assert!(e.contains("at line 3, column 9"), "{e}");
    let e = err("print(1)\nprint(2 < nil)");
    assert!(e.contains("at line 2, column 9"), "{e}");
}

// Globals are bound by slot when a program is linked, not when it is compiled, so a
// function can use a global defined after it, and redefining one reuses its slot.
#[test]
fn globals_bind_late_and_can_be_redefined() {
    let src = "fn show() { return later * 2 }
let later = 21
print(show())
let later = 50
print(show())";
    assert_eq!(out(src), ["42", "100"]);
    let e = err("fn early() { return missing }\nearly()");
    assert!(e.contains("missing is not defined"), "{e}");
}

// Strings order by Unicode code point, so the comparison is case-sensitive.
#[test]
fn strings_compare_by_code_point() {
    assert_eq!(
        out(r#"print("apple" < "banana", "b" > "a", "same" <= "same", "" < "a")"#),
        ["true true true true"]
    );
    assert_eq!(out(r#"print("B" < "a", "Zebra" < "apple")"#), ["true true"]);
    assert_eq!(out(r#"print("é" > "z", "10" < "9")"#), ["true true"]);
    let e = err(r#"print(1 < "a")"#);
    assert!(
        e.contains("< needs two numbers or two strings, not a number and a string"),
        "{e}"
    );
}

#[test]
fn in_asks_whether_something_is_there() {
    assert_eq!(
        out(r#"print(2 in [1, 2, 3], 5 in [1, 2], "b" in ["a", "b"], 1 in [])"#),
        ["true false true false"]
    );
    assert_eq!(
        out(r#"let m = {"name": "ada"}
print("name" in m, "born" in m)"#),
        ["true false"]
    );
    assert_eq!(
        out(r#"print("ell" in "hello", "xyz" in "hello", "" in "hello")"#),
        ["true false true"]
    );
    // A list is found by identity, as == finds it; the same list inside is found.
    assert_eq!(
        out("let inner = [1]\nprint(inner in [inner], [1] in [[1]])"),
        ["true false"]
    );
}

#[test]
fn in_binds_like_a_comparison() {
    // Arithmetic first, then in, then and/or; and `for x in xs` is still a loop.
    assert_eq!(
        out("print(1 + 1 in [2], 3 in [3] and 4 in [5], not (2 in [1]))"),
        ["true false true"]
    );
    assert_eq!(
        out("let n = 0\nfor x in [1, 2] { if x in [2] { n = n + 1 } }\nprint(n)"),
        ["1"]
    );
}

#[test]
fn in_says_what_it_needs() {
    assert!(err(r#"print(1 in {"a": 1})"#).contains("map keys must be strings, not a number"));
    assert!(
        err(r#"print(1 in "abc")"#)
            .contains("in a string needs a string to look for, not a number")
    );
    assert!(
        err("print(1 in 5)")
            .contains("in needs a list, a map or a string on its right, not a number")
    );
}
