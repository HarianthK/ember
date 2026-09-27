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
fn a_map_prints_in_key_order() {
    let src = r#"let m = {"b": 2, "a": [1, "x"], "c": {"d": nil}}
print(m)"#;
    assert_eq!(out(src), [r#"{"a": [1, "x"], "b": 2, "c": {"d": nil}}"#]);
    assert_eq!(out("print({})"), ["{}"]);
}

#[test]
fn keys_read_and_write() {
    let src = r#"let person = {"name": "ada"}
person["born"] = 1815
person["name"] = "Ada"
print(person["name"], person["born"], len(person))"#;
    assert_eq!(out(src), ["Ada 1815 2"]);
}

#[test]
fn a_field_is_a_string_key() {
    let src = r#"let p = {"name": "ada"}
p.born = 1815
p.name = p.name + " lovelace"
print(p.name, p["born"])
print(p)"#;
    assert_eq!(
        out(src),
        [
            "ada lovelace 1815",
            r#"{"born": 1815, "name": "ada lovelace"}"#
        ]
    );
    // Chained, and as an assignment expression with a value.
    assert_eq!(
        out(r#"let a = {"b": {"c": 1}}
print(a.b.c = 5)
print(a)"#),
        ["5", r#"{"b": {"c": 5}}"#]
    );
}

#[test]
fn a_map_is_shared_not_copied() {
    let src = r#"fn tag(m) { m.seen = true }
let item = {}
tag(item)
print(item)"#;
    assert_eq!(out(src), [r#"{"seen": true}"#]);
    assert_eq!(out("print({} == {})"), ["false"]);
}

#[test]
fn keys_and_has() {
    let src = r#"let m = {"z": 1, "a": 2}
print(keys(m))
print(has(m, "a"), has(m, "q"))
let total = 0
for k in keys(m) { total = total + m[k] }
print(total)"#;
    assert_eq!(out(src), [r#"["a", "z"]"#, "true false", "3"]);
}

#[test]
fn a_map_inside_itself_prints_without_recursing_forever() {
    assert_eq!(out("let m = {}\nm.me = m\nprint(m)"), [r#"{"me": {...}}"#]);
    assert_eq!(
        out("let m = {}\nm.list = [m]\nprint(m)"),
        [r#"{"list": [{...}]}"#]
    );
}

#[test]
fn a_closure_can_keep_state_in_a_map() {
    let src = r#"fn make_account(balance) {
  let account = {"balance": balance}
  account.deposit = fn(n) { account.balance = account.balance + n }
  return account
}
let acct = make_account(10)
acct.deposit(5)
acct.deposit(20)
print(acct.balance)"#;
    assert_eq!(out(src), ["35"]);
}

#[test]
fn map_mistakes_say_what_went_wrong() {
    let e = err("let m = {\"a\": 1}\nprint(m.b)");
    assert!(e.contains(r#"the map has no key "b" at line 2"#), "{e}");
    assert!(err("let m = {1: 2}").contains("map keys must be strings, not a number"));
    assert!(err("let m = {}\nm[1] = 2").contains("map keys must be strings, not a number"));
    assert!(err("for k in {} { }").contains("loop over keys(m) instead"));
    assert!(err("let n = 5\nprint(n.x)").contains("a number cannot be indexed"));
    assert!(
        err("let n = 5\nn.x = 1")
            .contains("only a list's or a map's items can be assigned, not a number's")
    );
    assert!(err("has([], \"a\")").contains("has needs a map, not a list"));
    assert!(err("keys(1)").contains("keys needs a map, not a number"));
}

#[test]
fn maps_leave_the_stack_clean() {
    let src = r#"let counts = {}
for word in ["a", "b", "a", "c", "a"] {
  if has(counts, word) { counts[word] = counts[word] + 1 } else { counts[word] = 1 }
}"#;
    let script = compile(&parse(src).unwrap()).unwrap();
    let mut vm = Vm::new();
    vm.run(script).unwrap();
    assert_eq!(vm.stack_depth(), 0);
}
