// The tree-walker and the VM share only the parser, so agreeing is a check neither can fake.
use ember::walk::Walker;

fn both(src: &str) -> Vec<String> {
    let vm = ember::run(src).unwrap_or_else(|e| panic!("VM on {src}: {e}"));
    let walked = Walker::run(src).unwrap_or_else(|e| panic!("tree-walker on {src}: {e}"));
    assert_eq!(vm, walked, "the two engines disagree on:\n{src}");
    vm
}

#[test]
fn the_engines_agree_on_arithmetic_and_logic() {
    assert_eq!(
        both("print(1 + 2 * 3, 10 - 3 - 2, 7 % 3, -2 * -3)"),
        ["7 5 1 6"]
    );
    assert_eq!(
        both("print(1 < 2 and 3 > 4, nil or 5, not 0)"),
        ["false 5 false"]
    );
}

#[test]
fn the_engines_agree_on_scope_and_shadowing() {
    let src = "let x = 1
{
  let x = 2
  {
    let x = 3
    print(x)
  }
  x = x + 10
  print(x)
}
print(x)";
    assert_eq!(both(src), ["3", "12", "1"]);
}

#[test]
fn the_engines_agree_on_recursion_and_loops() {
    let src = "fn fib(n) {
  if n < 2 { return n }
  return fib(n - 1) + fib(n - 2)
}
let i = 0
let total = 0
while i < 20 {
  total = total + fib(i)
  i = i + 1
}
print(total)";
    assert_eq!(both(src), ["10945"]);
}

#[test]
fn the_engines_agree_on_closures() {
    let src = "fn make(start) {
  let n = start
  return fn() {
    n = n + 1
    return n
  }
}
let a = make(0)
let b = make(100)
a()
print(a(), b(), a())";
    assert_eq!(both(src), ["2 101 3"]);
}

// A `let` inside an if or a loop body belongs to that block, not to the code around it.
#[test]
fn the_engines_agree_that_branches_and_loop_bodies_are_scopes() {
    let src = "let x = 1
if true { let x = 2 }
let i = 0
while i < 2 {
  let x = 3
  i = i + 1
}
print(x)";
    assert_eq!(both(src), ["1"]);
}
