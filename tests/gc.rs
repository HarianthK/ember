use ember::compiler::compile;
use ember::parser::parse;
use ember::vm::Vm;

fn run_in(vm: &mut Vm, src: &str) {
    let script = compile(&parse(src).unwrap()).unwrap();
    vm.run(script).unwrap_or_else(|e| panic!("{src}: {e}"));
}

#[test]
fn cycles_nothing_can_reach_are_freed() {
    // Reference counting could never free these: each list holds itself.
    let mut vm = Vm::new();
    run_in(
        &mut vm,
        "let i = 0
while i < 500 {
  let xs = [i]
  push(xs, xs)
  i = i + 1
}",
    );
    vm.collect();
    assert_eq!(
        vm.heap.live(),
        0,
        "every list was garbage once the loop moved on"
    );
}

#[test]
fn an_object_holding_a_closure_over_itself_is_freed() {
    // The account map holds a closure whose upvalue holds the account map.
    let mut vm = Vm::new();
    run_in(
        &mut vm,
        "fn make_account(balance) {
  let account = {\"balance\": balance}
  account.deposit = fn(n) { account.balance = account.balance + n }
  return account
}
let i = 0
while i < 200 {
  make_account(i).deposit(1)
  i = i + 1
}",
    );
    vm.collect();
    // Only make_account itself is left: a top-level function is a closure held by a global.
    assert_eq!(vm.heap.live(), 1);
}

#[test]
fn what_a_global_can_reach_survives() {
    let mut vm = Vm::new();
    run_in(
        &mut vm,
        "fn make_counter() {
  let n = 0
  return fn() { n = n + 1
    return n }
}
let kept = {\"counter\": make_counter(), \"rows\": [[1, 2], [3]]}
kept.counter()
let i = 0
while i < 5000 {
  let garbage = [[i], {\"k\": i}]
  i = i + 1
}
kept.counter()
print(kept.counter(), kept.rows)",
    );
    assert!(
        vm.heap.collections > 0,
        "the loop should have made the heap collect"
    );
    assert_eq!(vm.output, ["3 [[1, 2], [3]]"]);
    vm.collect();
    // The map, its counter closure, that closure's upvalue, the three lists, and
    // make_counter itself, which is a closure held by a global.
    assert_eq!(vm.heap.live(), 7);
}

#[test]
fn a_long_loop_runs_in_bounded_memory() {
    let mut vm = Vm::new();
    // This measures the normal collector; stress mode never reuses a slot, by design.
    vm.heap.stress = false;
    run_in(
        &mut vm,
        "let i = 0
while i < 100000 {
  let temporary = [i, [i]]
  i = i + 1
}",
    );
    // 200,000 lists were made; the heap never needed more than a few thousand slots.
    assert!(vm.heap.freed >= 190_000, "freed only {}", vm.heap.freed);
    assert!(
        vm.heap.slots_used() < 5_000,
        "the heap grew to {} slots",
        vm.heap.slots_used()
    );
}

// Once make() returns, the list lives only inside the closure's closed upvalue. No other
// test had a heap object reachable by that path alone, so untracing it went unnoticed.
#[test]
fn a_value_only_a_closure_remembers_survives() {
    let mut vm = Vm::new();
    run_in(
        &mut vm,
        "fn make() {
  let items = [1, [2, 3]]
  return fn() { return items }
}
let get = make()
let i = 0
while i < 3000 {
  let junk = [i]
  i = i + 1
}
print(get())",
    );
    assert!(
        vm.heap.collections > 0,
        "the loop should have made the heap collect"
    );
    assert_eq!(vm.output, ["[1, [2, 3]]"]);
}

// The closure is thrown away at once, so while x's block is still running the only thing
// holding its upvalue is the VM's list of open upvalues, which the block's end then closes.
#[test]
fn an_upvalue_whose_closure_was_dropped_survives_until_its_scope_ends() {
    let mut vm = Vm::new();
    vm.heap.stress = true;
    run_in(
        &mut vm,
        "{
  let x = [1]
  fn() { return x }
  let junk = [2]
  print(x)
}",
    );
    assert_eq!(vm.output, ["[1]"]);
}
