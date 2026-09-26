# ember

A programming language with its own bytecode virtual machine and garbage
collector, written in Rust with no dependencies. This is a long build, done in
phases; phase 1, the front end, is here.

```
let greeting = "hello";

fn fib(n) {
  if n < 2 { return n; }
  return fib(n - 1) + fib(n - 2);
}

let squares = [];
for n in [1, 2, 3, 4] {
  squares = squares + [n * n];
}

let person = {"name": "ada", "born": 1815};
person.name = "Ada";
```

## Where it is

- [x] **Phase 1, the front end.** Lexer with line and column on every token,
      parser by precedence climbing, an AST, and a printer that turns the tree
      back into source.
- [ ] **Phase 2, the compiler and VM.** Done so far: bytecode with a line for
      every instruction, a stack machine, constants, globals, locals as stack
      slots, `if`, `while`, and short-circuit `and`/`or`. FizzBuzz runs. Still
      to come: calls.
- [ ] **Phase 3, closures and objects.** Upvalues, captured environments,
      lists and maps as heap values.
- [ ] **Phase 4, the garbage collector.** Mark and sweep over an arena the VM
      owns, with the roots taken from the stack and the call frames.
- [ ] **Phase 5, the finish.** A REPL, a small standard library, error traces
      with line numbers, and benchmarks against a tree-walker.

## Running it

    cargo run -- examples/fizzbuzz.em           # runs it
    cargo run -- examples/fizzbuzz.em --dis     # shows the bytecode
    cargo run -- examples/tour.em --parse       # shows how the parser read it
    cargo test

The disassembly is a listing with the source line beside each instruction:

    0000    2 CONSTANT        0 (1)
    0001    | CONSTANT        1 (2)
    0002    | CONSTANT        2 (3)
    0003    | MUL
    0004    | ADD
    0005    | PRINT

## The language

Dynamically typed, expressions over statements where it can be. Values are
numbers (one float type), strings, booleans, `nil`, lists, maps and functions.
Functions are values, so `fn add(a, b) {}` is sugar for `let add = fn(a, b) {}`
and closures come free in phase 3. Semicolons are optional between statements.
`and`, `or` and `not` are words rather than symbols.

## Checking it

Nine tests covering the parts that are easy to get quietly wrong: two-character
operators, keywords that are only keywords alone, precedence and associativity,
assignment targets, chained calls and indexes, and where errors point. The last
one is a property: printing a parse and parsing it back must print the same
thing, which catches a printer that loses grouping and a parser that disagrees
with its own output.

Each test was proved able to fail by breaking one thing at a time: making
binary operators right associative, giving `*` the same binding power as `+`,
and letting anything be an assignment target. Each produced a different failure.

## Notes

[DOCS.md](DOCS.md) is the design reasoning and what each phase taught me.
