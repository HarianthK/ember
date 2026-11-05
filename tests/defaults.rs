use ember::ast::print_stmts;
use ember::compiler::compile;
use ember::parser::parse;
use ember::run;
use ember::vm::Vm;
use ember::walk::Walker;

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
fn a_left_out_argument_takes_its_default() {
    let src = "fn greet(name, greeting = \"hello\", mark = \"!\") {
  return greeting + \", \" + name + mark
}
print(greet(\"ada\"))
print(greet(\"ada\", \"hi\"))
print(greet(\"ada\", \"hi\", \"?\"))";
    assert_eq!(out(src), ["hello, ada!", "hi, ada!", "hi, ada?"]);
    // The tree-walker, which shares only the parser and runs only numbers, agrees.
    let sums = "fn f(a, b = 10, c = 100) { return a + b + c }\nprint(f(1))\nprint(f(1, 2))\nprint(f(1, 2, 3))";
    assert_eq!(out(sums), ["111", "103", "6"]);
    assert_eq!(Walker::run(sums).unwrap(), ["111", "103", "6"]);
}

#[test]
fn a_default_is_worked_out_at_each_call_and_can_read_earlier_parameters() {
    // A fresh list every call, not one shared list as in Python, and end defaults to len(xs).
    let src = "let calls = 0
fn tick() { calls += 1; return calls }
fn bag(item, into = []) { push(into, item); return into }
fn span(xs, start = 0, end = len(xs)) { return slice(xs, start, end) }
fn stamp(at = tick()) { return at }
print(bag(1), bag(2))
print(span([1, 2, 3, 4]), span([1, 2, 3, 4], 1), span([1, 2, 3, 4], 1, 2))
print(stamp(), stamp(), stamp(10), calls)";
    assert_eq!(
        out(src),
        ["[1] [2]", "[1, 2, 3, 4] [2, 3, 4] [2]", "1 2 10 2"]
    );
    assert_eq!(
        Walker::run("fn f(a, b = a * 2) { return b }\nprint(f(4), f(4, 1))").unwrap(),
        ["8 1"]
    );
}

#[test]
fn passing_nil_is_not_leaving_it_out() {
    assert_eq!(
        out("fn f(x = 1) { return x }\nprint(f(), f(nil))"),
        ["1 nil"]
    );
}

#[test]
fn defaults_work_in_closures_methods_and_recursion() {
    let src = "fn counter(step = 1) {
  let n = 0
  return fn(by = step) { n += by; return n }
}
let c = counter(10)
c()
c(5)
let m = {\"scale\": fn(x, k = 3) { return x * k }}
fn down(n, acc = []) {
  if n == 0 { return acc }
  push(acc, n)
  return down(n - 1, acc)
}
print(c(), m.scale(2), m.scale(2, 5), down(3))";
    assert_eq!(out(src), ["25 6 10 [3, 2, 1]"]);
}

#[test]
fn the_argument_count_is_checked_against_the_range() {
    assert!(
        err("fn f(a, b = 1) { }\nf()").starts_with("f takes 1 to 2 arguments, but was given 0"),
        "{}",
        err("fn f(a, b = 1) { }\nf()")
    );
    assert!(
        err("fn f(a, b = 1) { }\nf(1, 2, 3)")
            .starts_with("f takes 1 to 2 arguments, but was given 3")
    );
    assert!(err("fn f(a) { }\nf()").starts_with("f takes 1 argument, but was given 0"));
    // A required parameter cannot follow one with a default.
    let e = parse("fn f(a = 1, b) { }").unwrap_err().to_string();
    assert!(
        e.starts_with("b needs a default, since a parameter before it has one"),
        "{e}"
    );
    // A default that fails says so, from inside the call.
    assert!(err("fn f(x = nope) { return x }\nf()").starts_with("nope is not defined"));
}

#[test]
fn nothing_is_left_on_the_stack_and_defaults_print_back() {
    let script = compile(&parse("fn f(a, b = [a], c = {\"k\": b}) { return c }\nfor i in range(100) { f(i); f(i, 1); f(i, 1, 2) }").unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    assert_eq!(vm.stack_depth(), 0);
    let program = parse("fn f(a, b = 1 + 2, c = \"x\") { return a; }").unwrap();
    assert_eq!(
        print_stmts(&program, 0),
        "let f = fn f(a, b = (1 + 2), c = \"x\") {\n  return a;\n};"
    );
}
