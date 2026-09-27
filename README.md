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
- [x] **Phase 2, the compiler and VM.** Bytecode with a line for every
      instruction, a stack machine, globals, locals as stack slots, `if`,
      `while`, short-circuit `and`/`or`, functions with a frame per call,
      recursion, natives written in Rust, and errors that show the call stack.
- [x] **Phase 3, closures and objects.** Closures that capture variables and
      share them, lists, maps with fields, and `for` loops over lists and
      strings. Values are reference counted for now, which leaks cycles.
- [ ] **Phase 4, the garbage collector.** Mark and sweep over an arena the VM
      owns, with the roots taken from the stack and the call frames.
- [ ] **Phase 5, the finish.** A REPL, a small standard library, error traces
      with line numbers, and benchmarks against a tree-walker.

## Running it

    cargo run -- examples/tour.em               # runs it
    cargo run -- examples/counter.em --dis      # shows the bytecode
    cargo run -- examples/tour.em --parse       # shows how the parser read it
    cargo test

The disassembly is a listing with the source line beside each instruction.
Here `make_counter` returns a closure over its local `n`: `[local 1]` is the
closure capturing slot 1, and the inner function reads and writes it through
its upvalue 0.

    == make_counter ==
    0000    2 CONSTANT        0 (0)
    0001    3 CLOSURE         1 (<fn anonymous>) [local 1]
    0002    | RETURN

    == anonymous ==
    0000    4 GETUPVALUE      0
    0001    | CONSTANT        0 (1)
    0002    | ADD
    0003    | SETUPVALUE      0

## The language

Dynamically typed. Values are numbers (one float type), strings, booleans,
`nil`, lists, maps and functions. Functions are values and close over the
variables around them, so `fn add(a, b) {}` is sugar for `let add = fn(a, b) {}`.
Lists and maps are shared, not copied, as in Python. Map keys are strings, and
`m.name` is sugar for `m["name"]`. A missing key or an undefined name is an
error rather than a quiet `nil`. Semicolons are optional between statements, and
`and`, `or` and `not` are words, returning whichever operand decided, so
`name or "default"` works.

Built in: `print`, `len`, `push`, `keys`, `has` and `clock`.

## Checking it

81 tests, one file per part of the language: parsing, the chunk, running
expressions, calls, closures, lists and maps. Programs are run and their printed
output compared, and after every long program the stack must be empty, because
a value left behind on the stack changes nothing a program prints.

Every milestone was checked by breaking it on purpose, one thing at a time, and
watching for the test written to catch it. That process found tests too weak to
matter (one closure test could not tell shared variables from copies) and two
real bugs that had been there since the first phase: errors reported on the
wrong line, and every non-ASCII string silently corrupted. DOCS.md has both.

## Notes

[DOCS.md](DOCS.md) is the design reasoning and what each phase taught me.
