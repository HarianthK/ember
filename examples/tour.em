// Everything the language can say so far. Phase 1 parses this; the VM will run it.
let greeting = "hello";

fn fib(n) {
  if n < 2 {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

let squares = [];
for n in [1, 2, 3, 4] {
  squares = squares + [n * n];
}

let person = {"name": "ada", "born": 1815};
person.name = "Ada";

let counter = fn(start) {
  let n = start;
  return fn() {
    n = n + 1;
    return n;
  };
};

let i = 0;
while i < 10 and not done {
  i = i + 1;
}
