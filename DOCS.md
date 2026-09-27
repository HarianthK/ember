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

## Spans on everything that can fail

Every token carries a line and column, and so does every node that can fail at
run time. This section first claimed that was true from the start. It was not:
names and operators had no position of their own, and it took until phase 2 to
notice, because a stopgap was hiding it. See "The error that pointed at the
wrong line" below. Literals still carry none, since a constant cannot fail.

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

## Phase 2: calls

### One stack, many frames

Every call gets a frame: the function, where it is in that function's code,
and a base, the point on the value stack where its slot 0 sits. There is still
only one stack. The caller pushes the function and then the arguments, and the
new frame's base is simply where the function was, so the arguments are already
sitting in slots 1, 2, 3: the parameters. Nothing is copied. Returning truncates
the stack back to the base, which removes the function, its arguments and all
of its locals in one step, and pushes the result in their place.

The current frame's position is kept in plain local variables inside the run
loop, not in the frame, and written back only when another call happens. Every
instruction reads it, so it is worth not going through a vector for it.

### Slot 0 is the function itself

The function being called sits in slot 0 of its own frame, which is the usual
design. Naming that slot after the function, rather than leaving it blank,
buys recursion for free: `fact` calling `fact` resolves to slot 0 even when
`fact` is a local and closures do not exist yet. The one test that uses a local
recursive function fails the moment the slot is left unnamed.

### Reaching an outer local is refused, not guessed

A function inside another function cannot see the outer one's locals until
closures arrive in phase 3. The obvious implementation would treat such a name
as a global, and if a global of the same name happened to exist, would read it
and return the wrong value with no error at all. So each compiler is told the
names of every local around it, and using one is a compile error that says why.
The only exception is the enclosing function's own name, since a named function
is usually a global too and nested code may call it.

### Natives need no frame

`print` and `clock` are written in Rust. A native runs to completion the moment
it is called, so it gets no frame: its arguments are split off the stack,
handed over, and replaced by its result. `print` was a special instruction until
this existed, and removing that special case is what exposed the bug below.

### The error that pointed at the wrong line

Before this phase, `1 + "a"` on line 2 reported line 1. Binary operators and
names had no position in the syntax tree, so the instruction took whichever
position the compiler had last been given, and that could be the statement
before. The stopgap `print` instruction set its own position before compiling
its argument, which happened to hide the problem in every test, because every
test printed. Making `print` an ordinary call removed that, and the test for
error positions failed at once. Names, unary and binary operators now carry the
position of their token, and the error points at the exact operator:
`at line 2, column 3`.

### Traces

A runtime error now lists every call it happened inside, innermost first, each
at the line of the call it is waiting on. The run loop raises a plain fault;
the outer `run` builds the trace from the frames still on the stack, then
empties the machine so it can run the next program. Runaway recursion is ten
thousand identical frames, so the display collapses repeats into one line, as
Python does.

## Phase 3: closures and objects

### An upvalue is a pointer that becomes a box

A closure needs to keep variables that belong to a function which may already
have returned. The design here is Lua's. While the variable's scope is alive,
the closure's upvalue is open: it holds the stack slot and reads and writes
there, so the enclosing function and the closure see one variable. When the
scope ends, by a block closing or the function returning, the upvalue is
closed: the value moves off the stack into the upvalue itself. The closure
never notices the difference; every read goes through the upvalue either way.

Two closures that capture the same variable must share it, so the VM keeps a
list of open upvalues and reuses one if the slot already has it. The compiler
marks every captured local, and a block ending emits `CLOSEUPVALUE` for those
instead of `POP`. Each pass of a loop body is its own scope, so a closure made
inside a loop captures that pass's variable rather than a single one that
every closure would then share.

A variable two functions out reaches the innermost through the one in between:
the middle function captures it too, even if it never mentions it, and the
inner one captures the middle's upvalue. Each closure only ever looks one level
out, which is what keeps creation cheap.

### The test that could not fail

The first test for sharing called a setter while the function that owned the
variable was still running, and then read it back through a getter. Deliberately
breaking the VM so that every capture made a new upvalue did not fail it: while
the variable is still on the stack, two separate upvalues both point at the same
slot and agree. Sharing only shows after the scope has ended, when each separate
copy would hold its own value. The test now writes through one closure after the
owner has returned, and it is the only test that catches that break.

### for is a while loop with two hidden locals

`for x in xs` compiles to a counting loop: the list and a counter are kept in
two locals whose names start with a space. No name in a program can start with
a space, so the loop cannot be read or disturbed by the code inside it. The
length is read on every pass, so pushing to the list inside the loop extends
it, as in Python.

### Lists and maps are shared, and compared by identity

`let b = a` makes `b` the same list, and `push` changes it for every name that
holds it, as in Python. Equality is identity, as in Lua and JavaScript, because
a list can contain itself: `push(xs, xs)` is legal, and comparing contents would
never finish. The same cycle would make printing recurse forever, so printing
keeps track of what it is inside and shows `[...]`, as Python does.

Maps take string keys only, as JSON does, which avoids deciding what it means to
hash a float. They are a `BTreeMap`, so they always print in key order and test
output never depends on hash order. A missing key is an error, the same as an
undefined variable, and `has(m, k)` asks first. `m.name` is `m["name"]`: fields
compile to the same instructions as indexing with a constant key.

### The bug the é test found

A test that indexed `"héllo"` expected `é` and got `Ã`. The lexer reads the
source as bytes, and it had turned each byte of a string into a character on its
own. `é` is two bytes in UTF-8, so every string with an accent, a non-Latin
script or an emoji had been silently corrupted since phase 1, and `len("héllo")`
was 6. No test had used anything but ASCII. The lexer now collects a string's
bytes and decodes them once, and it no longer counts continuation bytes as
columns, so an error after an `é` points at the right place.

### Reference counting leaks cycles

Every heap value is an `Rc` for now, which frees a value when the last reference
to it goes. A cycle never reaches zero. Two of the tests build one on purpose:
a list pushed into itself, and an account map holding a closure that captured
the map. Both work, and both leak. That is the concrete reason for phase 4: a
collector that traces what is reachable from the stack and the globals frees a
cycle that nothing can reach, where counting never will.
