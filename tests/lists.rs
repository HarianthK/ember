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
fn lists_print_with_their_strings_quoted() {
    assert_eq!(
        out(r#"print([1, "two", nil, [3, []]])"#),
        [r#"[1, "two", nil, [3, []]]"#]
    );
    // A bare string is printed as itself; only inside a list does it get quotes.
    assert_eq!(out(r#"print("two")"#), ["two"]);
}

#[test]
fn indexing_reads_and_writes() {
    assert_eq!(out("let xs = [10, 20, 30]\nprint(xs[0], xs[2])"), ["10 30"]);
    assert_eq!(
        out("let xs = [10, 20]\nxs[1] = 99\nprint(xs)"),
        ["[10, 99]"]
    );
    // Index assignment is an expression with the assigned value, like any assignment.
    assert_eq!(out("let xs = [0]\nprint(xs[0] = 5)"), ["5"]);
    assert_eq!(
        out("let grid = [[1, 2], [3, 4]]\ngrid[1][0] = 7\nprint(grid)"),
        ["[[1, 2], [7, 4]]"]
    );
    assert_eq!(out(r#"print("héllo"[1])"#), ["é"]);
}

#[test]
fn a_list_is_shared_not_copied() {
    assert_eq!(
        out("let a = [1, 2]\nlet b = a\nb[0] = 9\nprint(a)"),
        ["[9, 2]"]
    );
    let src = "fn fill(xs) { push(xs, 1) }
let mine = []
fill(mine)
fill(mine)
print(mine, len(mine))";
    assert_eq!(out(src), ["[1, 1] 2"]);
}

#[test]
fn plus_joins_into_a_new_list() {
    let src = "let a = [1]
let b = [2, 3]
let c = a + b
c[0] = 0
print(a, b, c)";
    assert_eq!(out(src), ["[1] [2, 3] [0, 2, 3]"]);
}

#[test]
fn lists_are_equal_only_to_themselves() {
    assert_eq!(out("print([1] == [1])"), ["false"]);
    assert_eq!(out("let a = [1]\nlet b = a\nprint(a == b)"), ["true"]);
}

#[test]
fn a_list_inside_itself_prints_without_recursing_forever() {
    assert_eq!(out("let xs = [1]\npush(xs, xs)\nprint(xs)"), ["[1, [...]]"]);
}

#[test]
fn for_walks_a_list_or_a_string() {
    let src = "let total = 0
for n in [1, 2, 3, 4] {
  total = total + n
}
print(total)";
    assert_eq!(out(src), ["10"]);
    assert_eq!(out("for c in \"abc\" { print(c) }"), ["a", "b", "c"]);
    assert_eq!(out("for x in [] { print(x) }\nprint(\"done\")"), ["done"]);
    let squares = "let squares = []
for n in [1, 2, 3] {
  squares = squares + [n * n]
}
print(squares)";
    assert_eq!(out(squares), ["[1, 4, 9]"]);
}

#[test]
fn nested_loops_and_closures_see_their_own_item() {
    let src = "let pairs = []
for a in [1, 2] {
  for b in [\"x\", \"y\"] {
    push(pairs, [a, b])
  }
}
print(pairs)";
    assert_eq!(out(src), [r#"[[1, "x"], [1, "y"], [2, "x"], [2, "y"]]"#]);
    let src = "let getters = []
for n in [10, 20, 30] {
  push(getters, fn() { return n })
}
print(getters[0](), getters[1](), getters[2]())";
    assert_eq!(out(src), ["10 20 30"]);
}

#[test]
fn the_loop_counter_is_out_of_reach() {
    // The hidden counter is named " i"; a program's i is a different name.
    assert!(err("for x in [1] { print(i) }").contains("i is not defined"));
}

#[test]
fn list_mistakes_say_what_went_wrong() {
    let e = err("let xs = [1, 2, 3]\nprint(xs[3])");
    assert!(
        e.contains("index 3 is out of range for a list of length 3 at line 2"),
        "{e}"
    );
    assert!(err("print([1][-1])").contains("index -1 is out of range"));
    assert!(err("print([1][0.5])").contains("a list index must be a whole number, not 0.5"));
    assert!(err("print(5[0])").contains("a number cannot be indexed"));
    assert!(
        err("let s = \"ab\"\ns[0] = \"c\"")
            .contains("only a list's items can be assigned, not a string's")
    );
    assert!(err("for x in 5 { }").contains("for needs a list or a string, not a number"));
    assert!(err("push(1, 2)").contains("push needs a list, not a number"));
    assert!(err("print([1] + 1)").contains("+ needs two numbers, two strings or two lists"));
}

#[test]
fn loops_over_lists_leave_the_stack_clean() {
    let src = "let out = []
for row in [[1, 2], [3], []] {
  for x in row {
    let f = fn() { return x }
    push(out, f())
  }
}";
    let script = compile(&parse(src).unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    assert_eq!(vm.stack_depth(), 0);
}
