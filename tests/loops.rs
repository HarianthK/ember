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
fn break_leaves_a_while_loop() {
    let src = "let i = 0
while true {
  if i == 3 { break }
  i = i + 1
}
print(i)";
    assert_eq!(out(src), ["3"]);
    // Inside a function, the locals below the loop are still needed after it, so a break
    // that left the stack one value short would show here, where at the top level it did not.
    let src = "fn count() {
  let before = 10
  let i = 0
  while true {
    let step = 1
    i = i + step
    if i == 3 { break }
  }
  return before + i
}
print(count())";
    assert_eq!(out(src), ["13"]);
}

#[test]
fn continue_skips_to_the_next_pass() {
    let src = "let i = 0
let odd = []
while i < 7 {
  i = i + 1
  if i % 2 == 0 { continue }
  push(odd, i)
}
print(odd)";
    assert_eq!(out(src), ["[1, 3, 5, 7]"]);
}

#[test]
fn break_and_continue_work_in_for_loops() {
    let src = "let seen = []
for n in range(10) {
  if n == 6 { break }
  if n % 2 == 1 { continue }
  push(seen, n)
}
print(seen)";
    assert_eq!(out(src), ["[0, 2, 4]"]);
    // continue in a for loop must still advance the counter, or it would never end.
    assert_eq!(
        out("let n = 0\nfor c in \"abc\" { n = n + 1\n continue }\nprint(n)"),
        ["3"]
    );
}

#[test]
fn break_leaves_only_the_innermost_loop() {
    let src = "let pairs = []
for a in [1, 2, 3] {
  for b in [1, 2, 3] {
    if b > a { break }
    push(pairs, a * 10 + b)
  }
}
print(pairs)";
    assert_eq!(out(src), ["[11, 21, 22, 31, 32, 33]"]);
}

// A closure made in the body captured x; breaking out must close x, not just pop it,
// or the closure would later read whatever took over x's stack slot.
#[test]
fn a_closure_keeps_what_it_captured_after_a_break() {
    let src = "let keep = nil
let i = 0
while true {
  let x = i * 100
  keep = fn() { return x }
  if i == 2 { break }
  i = i + 1
}
let filler = \"something else on the stack\"
print(keep())";
    assert_eq!(out(src), ["200"]);
    let src = "let fs = []
for n in [1, 2, 3] {
  let doubled = n * 2
  push(fs, fn() { return doubled })
  if n == 2 { continue }
}
print(fs[0](), fs[1](), fs[2]())";
    assert_eq!(out(src), ["2 4 6"]);
}

#[test]
fn breaking_out_leaves_the_stack_clean() {
    let src = "let i = 0
while i < 1000 {
  let a = i
  let b = [a]
  i = i + 1
  if i % 3 == 0 { continue }
  if i == 998 { break }
}
for n in range(500) {
  let t = n
  if n == 250 { break }
  if n % 2 == 0 { continue }
}";
    let script = compile(&parse(src).unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    assert_eq!(vm.stack_depth(), 0);
}

#[test]
fn break_outside_a_loop_is_a_compile_error() {
    assert!(err("break").contains("break is only allowed inside a loop"));
    assert!(err("if true { continue }").contains("continue is only allowed inside a loop"));
    // A function is its own world: a break inside it cannot leave the loop around it.
    let e = err("while true {\n  fn f() { break }\n  break\n}");
    assert!(
        e.contains("break is only allowed inside a loop at line 2"),
        "{e}"
    );
}
