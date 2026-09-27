// Run: cargo run --release --bin bench. Prints a markdown table; DOCS.md has the results.
use ember::walk::Walker;
use std::process::Command;
use std::time::Instant;

const PROGRAMS: [(&str, &str, &str); 3] = [
    (
        "fib(27), recursion",
        "fn fib(n) {
  if n < 2 { return n }
  return fib(n - 1) + fib(n - 2)
}
print(fib(27))",
        "def fib(n):
    if n < 2: return n
    return fib(n - 1) + fib(n - 2)
print(fib(27))",
    ),
    (
        "3M-step loop over locals",
        "fn count(limit) {
  let i = 0
  let total = 0
  while i < limit {
    total = total + i % 7
    i = i + 1
  }
  return total
}
print(count(3000000))",
        "def count(limit):
    i = 0
    total = 0
    while i < limit:
        total = total + i % 7
        i = i + 1
    return total
print(count(3000000))",
    ),
    (
        "closure called 1M times",
        "fn make() {
  let n = 0
  return fn() {
    n = n + 1
    return n
  }
}
let c = make()
let i = 0
while i < 1000000 {
  c()
  i = i + 1
}
print(c())",
        "def make():
    n = 0
    def inc():
        nonlocal n
        n = n + 1
        return n
    return inc
c = make()
i = 0
while i < 1000000:
    c()
    i = i + 1
print(c())",
    ),
];

// The middle of three runs, so one slow run from the machine being busy does not count.
fn median_secs(mut run: impl FnMut() -> Vec<String>) -> (f64, Vec<String>) {
    let mut times = Vec::new();
    let mut out = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        out = run();
        times.push(start.elapsed().as_secs_f64());
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (times[1], out)
}

// Python times itself, so its interpreter's start-up is not counted against it.
fn python_secs(src: &str) -> Option<(f64, String)> {
    let timed =
        format!("import time\n_t = time.perf_counter()\n{src}\nprint(time.perf_counter() - _t)");
    let mut times = Vec::new();
    let mut answer = String::new();
    for _ in 0..3 {
        let out = Command::new("python").args(["-c", &timed]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let mut lines = text.lines();
        answer = lines.next()?.trim().to_string();
        times.push(lines.next()?.trim().parse::<f64>().ok()?);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some((times[1], answer))
}

fn main() {
    println!("| program | ember VM | tree-walker | VM speed-up | CPython |");
    println!("| --- | --- | --- | --- | --- |");
    for (name, src, python) in PROGRAMS {
        let (vm, vm_out) = median_secs(|| ember::run(src).expect("the VM runs it"));
        let (walk, walk_out) = median_secs(|| Walker::run(src).expect("the tree-walker runs it"));
        // Two engines that share only the parser must print the same answer.
        assert_eq!(
            vm_out, walk_out,
            "{name}: the VM and the tree-walker disagree"
        );
        let py = match python_secs(python) {
            Some((secs, answer)) => {
                assert_eq!(vm_out, [answer], "{name}: Python disagrees");
                format!("{:.2}s", secs)
            }
            None => "not installed".into(),
        };
        println!(
            "| {name} | {vm:.2}s | {walk:.2}s | {:.1}x | {py} |",
            walk / vm
        );
    }
}
