use ember::ast::print_stmts;
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

// Runs a program and returns how many values it left on the stack, which must be none.
fn leftover(src: &str) -> usize {
    let script = compile(&parse(src).unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    vm.stack_depth()
}

#[test]
fn a_runtime_error_runs_the_catch_with_its_message() {
    let src = "try {
  print(\"before\")
  print([1, 2][5])
  print(\"never\")
} catch e {
  print(\"caught\", e)
}
print(\"after\")";
    assert_eq!(
        out(src),
        [
            "before",
            "caught index 5 is out of range for a list of length 2",
            "after"
        ]
    );
    // No error, no catch.
    assert_eq!(out("try { print(1) } catch e { print(2) }"), ["1"]);
}

#[test]
fn throw_hands_over_any_value() {
    let src = "try { throw {\"code\": 404, \"why\": \"gone\"} } catch e { print(e.code, e.why) }";
    assert_eq!(out(src), ["404 gone"]);
    assert_eq!(out("try { throw [1, 2] } catch e { print(len(e)) }"), ["2"]);
    // Uncaught, the value is the message, with the line it was thrown from.
    assert_eq!(
        err("print(1)\nthrow \"bad input\""),
        "bad input at line 2, column 1\n  in script, line 2"
    );
}

#[test]
fn an_error_deep_in_calls_unwinds_to_the_try() {
    let src = "fn down(n) {
  let mine = n * 2
  if n == 0 { throw \"bottom\" }
  return down(n - 1) + mine
}
fn outer() {
  let keep = \"kept\"
  try { down(40) } catch e { print(e, keep) }
  return keep
}
print(outer())";
    assert_eq!(out(src), ["bottom kept", "kept"]);
    // The forty frames and their locals are gone, not left on the stack.
    assert_eq!(leftover(&src.replace("print(outer())", "outer()")), 0);
    // Runaway recursion is an error like any other, so it can be caught too.
    assert_eq!(
        out("fn forever() { forever() }\ntry { forever() } catch e { print(e) }\nprint(\"fine\")"),
        [
            "stack overflow: more than 10000 calls deep, in forever",
            "fine"
        ]
    );
}

#[test]
fn the_innermost_try_catches_and_a_catch_can_throw_again() {
    let src = "try {
  try { throw \"inner\" } catch e { print(\"first\", e); throw e + \" again\" }
} catch e {
  print(\"second\", e)
}";
    assert_eq!(out(src), ["first inner", "second inner again"]);
    // An error in a catch with no try around it is not caught by the try it belongs to.
    assert!(err("try { throw 1 } catch e { throw 2 }").starts_with("2 at line 1"));
}

#[test]
fn leaving_a_try_early_takes_its_handler_with_it() {
    // A try left by return, break or continue must not catch errors that happen afterwards.
    // g runs at the depth f's try had, which is where a stale handler would fire.
    assert_eq!(
        err(
            "fn f() { try { return 1 } catch e { print(\"f caught\") } }\nfn g() { return [][0] }\nf()\ng()"
        ),
        "index 0 is out of range for a list of length 0 at line 2, column 19\n  in g, line 2\n  in script, line 4"
    );
    assert!(
        err("while true { try { break } catch e { print(\"loop caught\") } }\nprint([][0])")
            .starts_with("index 0 is out of range")
    );
    assert!(
        err("for i in range(2) { try { continue } catch e { print(\"loop caught\") } }\nprint([][0])")
            .starts_with("index 0 is out of range")
    );
    // A try outside the loop is still open after a break out of an inner one.
    let src = "try {
  for i in range(3) {
    try { if i == 1 { break } } catch e { print(\"inner\") }
  }
  throw \"after the loop\"
} catch e { print(\"outer\", e) }";
    assert_eq!(out(src), ["outer after the loop"]);
    // The handler is the try's own frame: a return through it from inside a callee is fine.
    let src = "fn safe(f) { try { return f() } catch e { return \"failed: \" + e } }
print(safe(fn() { return 5 }))
print(safe(fn() { return nope }))
print(safe(fn() { return 6 }))";
    assert_eq!(out(src), ["5", "failed: nope is not defined", "6"]);
}

#[test]
fn closures_made_inside_a_try_keep_their_variables() {
    let src = "let fs = []
try {
  let x = 10
  push(fs, fn() { return x })
  throw \"leave\"
} catch e {
  print(fs[0]())
}";
    assert_eq!(out(src), ["10"]);
}

#[test]
fn the_stack_is_clean_after_a_catch() {
    let src = "fn f() {
  let a = 1
  for i in range(10) {
    let b = i
    try { let c = [b]; if i % 3 == 0 { throw c } } catch e { let d = e[0] }
  }
  return a
}
let i = 0
while i < 50 { f(); i = i + 1 }";
    assert_eq!(leftover(src), 0);
}

#[test]
fn try_prints_back_as_it_was_written() {
    let program = parse("try { throw \"x\"; } catch e { print(e); }").unwrap();
    assert_eq!(
        print_stmts(&program, 0),
        "try {\n  throw \"x\";\n} catch e {\n  print(e);\n}"
    );
    // catch needs a name to put the error in.
    assert!(parse("try { } catch { }").is_err());
}
