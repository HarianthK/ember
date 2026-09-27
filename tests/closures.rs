use ember::compiler::compile;
use ember::parser::parse;
use ember::run;
use ember::vm::Vm;

fn out(src: &str) -> Vec<String> {
    run(src).unwrap_or_else(|e| panic!("{src}: {e}"))
}

#[test]
fn a_closure_keeps_its_variable_after_the_function_returns() {
    let src = "fn make_counter() {
  let n = 0
  return fn() {
    n = n + 1
    return n
  }
}
let a = make_counter()
let b = make_counter()
print(a(), a(), a())
print(b())";
    // Each call to make_counter gets its own n, and it outlives the call that made it.
    assert_eq!(out(src), ["1 2 3", "1"]);
}

#[test]
fn two_closures_over_one_variable_share_it() {
    // The write happens after pair has returned, so value is off the stack by then. Two
    // separate copies would each close over "start" and get would never see the change.
    let src = "let setter = nil
fn pair() {
  let value = \"start\"
  setter = fn(v) { value = v }
  return fn() { return value }
}
let get = pair()
setter(\"changed after return\")
print(get())";
    assert_eq!(out(src), ["changed after return"]);
}

#[test]
fn a_closure_writes_the_enclosing_variable_while_it_is_still_live() {
    let src = "fn outer() {
  let x = 1
  let bump = fn() { x = x + 1 }
  bump()
  bump()
  return x
}
print(outer())";
    // While outer is running, x is still on the stack, and bump must write that slot.
    assert_eq!(out(src), ["3"]);
}

#[test]
fn each_pass_of_a_loop_captures_its_own_variable() {
    let src = "let first = nil
let second = nil
let i = 0
while i < 2 {
  let j = i
  if i == 0 { first = fn() { return j } } else { second = fn() { return j } }
  i = i + 1
}
print(first(), second())";
    // j ends with each pass; if it were not closed then, both would see the same slot.
    assert_eq!(out(src), ["0 1"]);
}

#[test]
fn capture_reaches_through_several_functions() {
    let src = "fn outer() {
  let x = \"outer x\"
  fn middle() {
    fn inner() { return x }
    return inner
  }
  return middle()
}
print(outer()())";
    // middle never mentions x, yet it has to carry it for inner.
    assert_eq!(out(src), ["outer x"]);
}

#[test]
fn a_captured_local_wins_over_a_global_of_the_same_name() {
    let src = "let x = \"global\"
fn outer() {
  let x = \"local\"
  fn inner() { return x }
  return inner()
}
print(outer())";
    assert_eq!(out(src), ["local"]);
}

#[test]
fn closures_are_compiled_as_captures() {
    let src = "fn outer() {
  let x = 1
  { let y = 2
    let f = fn() { return x + y } }
  return x
}";
    let script = compile(&parse(src).unwrap()).unwrap();
    let text = script.chunk.disassemble();
    assert!(
        text.contains("CLOSURE") && text.contains("[local 1, local 2]"),
        "{text}"
    );
    assert!(text.contains("GETUPVALUE"), "{text}");
    // y is captured and its block ends inside outer, so it is closed there, not popped.
    assert!(text.contains("CLOSEUPVALUE"), "{text}");
}

#[test]
fn closures_leave_the_stack_clean() {
    let src = "fn adder(n) { return fn(x) { return x + n } }
let i = 0
let total = 0
while i < 200 {
  let add = adder(i)
  { let k = i
    let f = fn() { return k }
    total = total + add(f()) }
  i = i + 1
}";
    let script = compile(&parse(src).unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    assert_eq!(vm.stack_depth(), 0);
}
