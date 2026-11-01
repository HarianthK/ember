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

fn leftover(src: &str) -> usize {
    let script = compile(&parse(src).unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    vm.stack_depth()
}

#[test]
fn each_operator_updates_a_name() {
    let src = "let x = 5
x += 3
x -= 1
x *= 4
x /= 2
x %= 5
print(x)";
    assert_eq!(out(src), ["4"]);
    // + joins strings and lists, so += does too, and the result is the value of the expression.
    assert_eq!(out("let s = \"a\"\ns += \"b\"\nprint(s += \"c\", s)"), ["abc abc"]);
    assert_eq!(out("let xs = [1]\nxs += [2, 3]\nprint(xs)"), ["[1, 2, 3]"]);
    // The tree-walker, which shares only the parser, gets the same answer.
    assert_eq!(Walker::run(src).unwrap(), ["4"]);
}

#[test]
fn locals_globals_and_captured_variables() {
    let src = "let total = 0
fn add(n) { total += n }
fn counter() {
  let n = 0
  return fn() { n += 1; return n }
}
fn sum(xs) {
  let s = 0
  for x in xs { s += x }
  return s
}
let next = counter()
next()
next()
add(5)
add(sum([1, 2, 3]))
print(total, next())";
    assert_eq!(out(src), ["11 3"]);
}

#[test]
fn items_keys_and_fields() {
    let src = "let xs = [1, 2, 3]
xs[1] += 10
xs[-1] *= 2
let m = {\"a\": 4}
m[\"a\"] -= 1
let p = {\"count\": 0, \"inner\": {\"hits\": [0, 0]}}
p.count += 1
p.count += 1
p.inner.hits[1] += 7
print(xs, m.a, p.count, p.inner.hits)";
    assert_eq!(out(src), ["[1, 12, 6] 3 2 [0, 7]"]);
}

#[test]
fn the_target_and_index_are_worked_out_once() {
    // Written out by hand as xs[i] = xs[i] + 1, pick() and at() would each run twice.
    let src = "let calls = 0
let xs = [10, 20]
fn pick() { calls += 1; return xs }
fn at() { calls += 100; return 1 }
pick()[at()] += 5
let m = {\"k\": 1}
fn which() { calls += 10000; return m }
which().k *= 3
print(xs, m.k, calls)";
    assert_eq!(out(src), ["[10, 25] 3 10101"]);
    // Nothing is left on the stack after many of them, in a function and at the top.
    assert_eq!(
        leftover("let xs = [0, 0]\nfn f() { let m = {\"n\": 0}; for i in range(50) { xs[i % 2] += i; m.n += 1 } }\nf()\nf()"),
        0
    );
}

#[test]
fn errors_name_what_is_missing() {
    assert!(err("nope += 1").starts_with("nope is not defined"), "{}", err("nope += 1"));
    assert!(err("let m = {}\nm.count += 1").contains("count"), "{}", err("let m = {}\nm.count += 1"));
    assert!(err("let xs = [1]\nxs[3] += 1").contains("out of range"));
    assert!(err("let s = \"a\"\ns -= 1").starts_with("- needs two numbers, not a string and a number"));
    // Only a name, an item or a field can be updated, and the two characters must touch.
    assert!(parse("1 += 2").is_err());
    assert!(parse("let x = 1\nx + = 2").is_err());
}

#[test]
fn updates_print_back_as_written() {
    let program = parse("x += 1; xs[0] -= 2; p.n *= 3; q /= 4; r %= 5; y = 6;").unwrap();
    assert_eq!(
        print_stmts(&program, 0),
        "(x += 1);\n(xs[0] -= 2);\n(p.n *= 3);\n(q /= 4);\n(r %= 5);\n(y = 6);"
    );
}
