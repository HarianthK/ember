# Notes on ember

## Why a bytecode VM and not a tree-walker

Walking the AST is the easy way to run a language, and it is slow for a reason
worth knowing: every step chases pointers around the heap, and the shape of the
work changes at every node, so the processor can predict nothing. Compiling to
a flat array of bytes and running a loop over it puts the work in one place,
the operands next to each other, and the dispatch in one switch. The plan here
is the second one, because the interesting parts of this project, the calling
convention and the garbage collector, only exist in that design.

## Precedence climbing instead of a grammar per level

A textbook parser writes one function per precedence level: expression calls
equality calls comparison calls term calls factor calls unary calls primary.
That is seven functions that differ only in a table. Precedence climbing keeps
one function and a table of binding powers, and the levels come from a number.
Left associativity falls out of asking for `power + 1` on the right hand side,
which stops the right side swallowing an operator of the same level; asking for
`power` instead would make `10 - 3 - 2` parse as `10 - (3 - 2)`, which is one of
the deliberate breakages the tests catch.

## Assignment is the exception

`a = b` looks like a binary operator but is not one: the left side is a place,
not a value, and it is right associative. So assignment is parsed above the
binary loop, and the left side is checked afterwards: a name, an index or a
field is a place, anything else is an error the parser can explain. Doing it
this way, rather than with a separate grammar rule, keeps `xs[i + 1].name = v`
working without any special case.

## Named functions are sugar

`fn add(a, b) { }` becomes `let add = fn add(a, b) { }`. One node type for all
functions means closures, higher order functions and recursion need no extra
machinery later: the inner name stays on the function so it can call itself and
so stack traces have something to print. The parser decides which it is by
looking one token past `fn`, which is the only lookahead in the whole parser.

## Where the printer earns its place

The AST printer exists to make the parser testable without asserting on tree
shapes, which are verbose and break whenever a field is added. Printing with
brackets around every binary node makes grouping visible, so a precedence test
reads as `assert_eq!(expr("1 + 2 * 3"), "(1 + (2 * 3))")`. It also gives a
property worth more than the individual cases: print, parse, print again, and
the two strings must match.

Comparing the parsed trees directly does not work, and the reason is a useful
one: the reprinted source sits on different lines, so the spans rightly differ.
The test compares what the trees mean, not where they came from.

The expression printer carries the indentation of the statement holding it, so
a function literal nested inside another lines its body up properly. It did not
at first, and nothing caught it: the round trip still passed, because the output
reparsed to the same thing. Only reading the printed sample showed it.

## Spans on everything from the start

Every token carries a line and column, and the nodes that can fail at runtime
carry one too. Adding that later means threading a parameter through every
function in the compiler, so it is cheaper to pay for it now, even though phase
1 only uses it for parse errors. The runtime will need it to say which line
threw.

## The `{` problem

A brace starts both a block and a map literal, so `{}` at the start of a
statement is ambiguous. The rule here is the usual one: in statement position a
brace is a block, and a map literal there needs a parenthesis or an assignment
in front of it. Worth knowing it is a choice rather than an oversight.

## Phase 2: the virtual machine

### Instructions are an enum, not bytes

Real bytecode is a byte array: an opcode byte, then its operands packed after
it, and the run loop reads bytes and decodes. Here an instruction is a Rust
enum, `Op::Constant(u16)`, stored in a `Vec<Op>`. That costs space, four bytes
for every instruction where most need one, and buys two things: the operands
can never be misread, since an instruction and its operand cannot come apart,
and the compiler refuses to build until every instruction is handled in the run
loop, which is exactly what happened when variables were added. The dispatch is
the same either way, a `match` that compiles to a jump table. If the space ever
matters it is a contained change: encode to bytes at the end of compilation.

### Lines sit beside the code, not in it

Every instruction has a span in a parallel array. The run loop never touches it
until something goes wrong, and then the index of the failing instruction gives
its line. That is how `1 + "a"` on line 2 reports line 2 without the fast path
paying for it.

### Locals are the stack

A global is found by name in a hash map every time it is used. A local is not
looked up at all: the compiler knows that the third local declared is the third
value on the stack, so `GETLOCAL 2` is all the instruction says. Declaring a
local emits nothing, because the value its initialiser just computed is already
sitting in the right slot. Leaving a block pops one value per local, and that is
the local ending. The compiler keeps the list of names only while it compiles;
at run time names no longer exist.

Resolving from the end of that list is what makes an inner `x` hide an outer
one. Searching from the front instead finds the outermost `x`, and only the
shadowing test notices.

### A jump is an address that is filled in later

`if` compiles to: the condition, a jump past the `then` block if false, the
block, a jump past the `else` block, the `else` block. When the first jump is
emitted, the compiler does not know how long the `then` block will be, so it
writes a placeholder and comes back to patch it once it does. Jumps here name
the instruction to go to rather than a distance, so the same `Jump` goes
backwards to the top of a `while` loop.

`and` and `or` are jumps too, which is what makes them short-circuit: `false
and x` jumps past `x` without evaluating it, and the tests prove it by making
`x` a name that does not exist.

### The bug output cannot see

The conditional jump leaves the condition on the stack, and each path pops it.
Deleting the pop on the `else` path makes every test that checks output still
pass: the program prints the right things, it just leaves one stray value
behind on every pass through the `else`. In a loop that is a stack growing by
one per iteration. The only test that caught it was one that runs a thousand
iterations of mixed branches and then asserts the stack is empty. A leak does
not change behaviour until it does, so the stack depth is checked directly.
