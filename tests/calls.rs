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
fn a_function_returns_its_value() {
    assert_eq!(
        out("fn add(a, b) { return a + b }\nprint(add(2, 3))"),
        ["5"]
    );
    // Arguments bind in order, so a subtraction shows it.
    assert_eq!(
        out("fn sub(a, b) { return a - b }\nprint(sub(10, 3))"),
        ["7"]
    );
}

#[test]
fn falling_off_the_end_returns_nil() {
    assert_eq!(out("fn f() {}\nprint(f())"), ["nil"]);
    assert_eq!(out("fn f() { return }\nprint(f())"), ["nil"]);
    assert_eq!(
        out("fn f(x) { if x { return 1 } }\nprint(f(false))"),
        ["nil"]
    );
}

#[test]
fn functions_are_values() {
    let src = "fn add(a, b) { return a + b }
let also = add
print(also(1, 1))
let square = fn(x) { return x * x }
print(square(4))
print(add)
print(add == also)";
    assert_eq!(out(src), ["2", "16", "<fn add>", "true"]);
    // Two functions with the same code are still two functions.
    assert_eq!(
        out("let a = fn() {}\nlet b = fn() {}\nprint(a == b)"),
        ["false"]
    );
}

#[test]
fn recursion_works() {
    let src = "fn fib(n) {
  if n < 2 { return n }
  return fib(n - 1) + fib(n - 2)
}
print(fib(20))";
    assert_eq!(out(src), ["6765"]);
}

#[test]
fn a_local_function_can_call_itself() {
    // fact is a local here, not a global, and closures do not exist yet. It still
    // works because slot 0 of every call holds the function being run, under its name.
    let src = "{
  fn fact(n) {
    if n <= 1 { return 1 }
    return n * fact(n - 1)
  }
  print(fact(10))
}";
    assert_eq!(out(src), ["3628800"]);
}

#[test]
fn each_call_gets_its_own_locals() {
    let src = "fn f(n) {
  let doubled = n * 2
  if n > 0 { f(n - 1) }
  return doubled
}
print(f(3))";
    // If the recursive calls shared doubled, the outermost would return 0.
    assert_eq!(out(src), ["6"]);
    let src = "{
  let a = 10
  fn twice(b) { let c = b * 2
    return c }
  print(twice(a) + a)
}";
    assert_eq!(out(src), ["30"]);
}

#[test]
fn parameters_are_copies() {
    let src = "fn bump(x) { x = x + 1
  return x }
let n = 1
print(bump(n))
print(n)";
    assert_eq!(out(src), ["2", "1"]);
}

#[test]
fn wrong_calls_say_what_went_wrong() {
    let e = err("fn add(a, b) { return a + b }\nadd(1)");
    assert!(e.contains("add takes 2 arguments, but was given 1"), "{e}");
    assert!(e.contains("line 2"), "{e}");
    assert!(err("fn one(a) {}\none()").contains("one takes 1 argument, but"));
    assert!(err("let n = 3\nn()").contains("a number cannot be called"));
    assert!(err("nil()").contains("a nil cannot be called"));
}

#[test]
fn compile_time_mistakes_are_caught() {
    assert!(err("return 1").contains("return is only allowed inside a function"));
    assert!(err("fn f(a, a) {}").contains("the parameter a is repeated"));
    assert!(err("fn f(a) { let a = 2 }").contains("a is already declared"));
    // Reaching into the enclosing function needs closures. Without the check this would
    // quietly read the global x and print "global".
    let e = err(
        "let x = \"global\"\nfn outer() {\n  let x = \"local\"\n  fn inner() { return x }\n  return inner()\n}\nprint(outer())",
    );
    assert!(e.contains("x belongs to an enclosing function"), "{e}");
}

#[test]
fn a_nested_function_can_still_call_globals() {
    let src = "fn helper() { return 7 }
fn outer() {
  fn inner() { return helper() + 1 }
  return inner()
}
print(outer())";
    assert_eq!(out(src), ["8"]);
}

#[test]
fn runaway_recursion_stops() {
    let e = err("fn forever(n) { return forever(n + 1) }\nforever(0)");
    assert!(e.contains("stack overflow"), "{e}");
    assert!(e.contains("forever"), "{e}");
}

#[test]
fn calls_leave_the_stack_clean() {
    let src = "fn add(a, b) { let sum = a + b
  return sum }
let i = 0
let total = 0
while i < 1000 {
  total = add(total, i)
  i = i + 1
}";
    let script = compile(&parse(src).unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    assert_eq!(vm.stack_depth(), 0);
}
