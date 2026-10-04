use crate::chunk::{Function, Native, Op, Value};
use crate::heap::{Closure, Heap, Obj, Ref, Upvalue};
use crate::lexer::Span;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

// map, filter and reduce, which take functions and so are written in ember.
const PRELUDE: &str = include_str!("prelude.em");

// Deep enough for any honest recursion, shallow enough to stop a runaway one quickly.
const MAX_FRAMES: usize = 10_000;

// A function call in progress. `base` is where its slot 0 sits on the shared stack. The
// function is kept beside its closure because every instruction reads it.
struct Frame {
    closure: Ref,
    function: Rc<Function>,
    ip: usize,
    base: usize,
}

// What the run loop raises: what went wrong, and the index of the instruction that failed
// in the running function. run() turns that into a line, so no instruction pays for it.
struct Fault {
    message: String,
    at: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    pub at: Span,
    // Empty when the fault is in the program itself, "prelude" when inside map and friends.
    pub source: &'static str,
    // Innermost call first: the function's name, its source and the line it had reached.
    pub trace: Vec<(String, &'static str, u32)>,
}

// "line 6", or "prelude line 6" for code that is not the program's own.
fn place(source: &str) -> String {
    if source.is_empty() {
        String::new()
    } else {
        format!("{source} ")
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}{}", self.message, place(self.source), self.at)?;
        // Runaway recursion is ten thousand identical frames; say so once, as Python does.
        let mut i = 0;
        while i < self.trace.len() {
            let (name, source, line) = &self.trace[i];
            let repeats = self.trace[i..]
                .iter()
                .take_while(|entry| *entry == &self.trace[i])
                .count();
            write!(f, "\n  in {name}, {}line {line}", place(source))?;
            if repeats > 1 {
                write!(
                    f,
                    "\n  ... the line above repeated {} more times",
                    repeats - 1
                )?;
            }
            i += repeats;
        }
        Ok(())
    }
}

pub struct Vm {
    stack: Vec<Value>,
    frames: Vec<Frame>,
    // Globals live in numbered slots; the name table is how link() finds a name's slot
    // and how an error names a global that was never defined (its slot still None).
    globals: Vec<Option<Value>>,
    global_names: Vec<String>,
    global_slots: HashMap<String, u16>,
    // Upvalues still pointing at a live stack slot, so a second capture of the same slot
    // finds and shares the first rather than making a copy.
    open_upvalues: Vec<Ref>,
    pub heap: Heap,
    // What print writes, so tests can read a program's output without capturing stdout.
    pub output: Vec<String>,
    pub echo: bool,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    pub fn new() -> Self {
        let mut vm = Vm {
            stack: Vec::with_capacity(256),
            frames: Vec::new(),
            globals: Vec::new(),
            global_names: Vec::new(),
            global_slots: HashMap::new(),
            open_upvalues: Vec::new(),
            heap: Heap::default(),
            output: Vec::new(),
            echo: false,
        };
        for native in NATIVES {
            let slot = vm.global_slot(native.name) as usize;
            vm.globals[slot] = Some(Value::Native(Rc::new(native)));
        }
        // EMBER_STRESS_GC=1 cargo test runs every test with a collection before every
        // instruction that follows an allocation.
        vm.heap.stress = std::env::var_os("EMBER_STRESS_GC").is_some();
        let prelude = crate::parser::parse(PRELUDE).expect("the prelude parses");
        let prelude =
            crate::compiler::compile_from(&prelude, "prelude").expect("the prelude compiles");
        vm.run(prelude).expect("the prelude runs");
        vm
    }

    // For tests: after a whole program the stack must be empty, or something leaked.
    pub fn stack_depth(&self) -> usize {
        self.stack.len()
    }

    fn pop(&mut self) -> Value {
        // The compiler balances every push with a pop, so an empty stack here is a compiler bug.
        self.stack
            .pop()
            .expect("the compiler emitted a pop with nothing on the stack")
    }

    // Arithmetic and comparison on two numbers, done in place: the right operand is popped
    // and the left one is overwritten with the result, one trip to the stack instead of three.
    // Ordering two numbers, or two strings by Unicode code point, in place like arith. A NaN
    // is unordered, so every comparison with one is false, as IEEE floats require.
    fn compare(&mut self, op: &str, at: usize, test: fn(Ordering) -> bool) -> Result<(), Fault> {
        let b = self.pop();
        let a = self.stack.last_mut().expect("a left operand");
        let order = match (&*a, &b) {
            (Value::Number(x), Value::Number(y)) => x.partial_cmp(y),
            (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
            _ => {
                return Err(Fault {
                    message: format!(
                        "{op} needs two numbers or two strings, not a {} and a {}",
                        a.type_name(),
                        b.type_name()
                    ),
                    at,
                });
            }
        };
        *a = Value::Bool(order.is_some_and(test));
        Ok(())
    }

    fn arith(
        &mut self,
        op: &str,
        at: usize,
        f: fn(f64, f64) -> Result<Value, &'static str>,
    ) -> Result<(), Fault> {
        let b = self.pop();
        let a = self.stack.last_mut().expect("a left operand");
        let result = match (&*a, &b) {
            (Value::Number(x), Value::Number(y)) => f(*x, *y),
            _ => {
                return Err(Fault {
                    message: format!(
                        "{op} needs two numbers, not a {} and a {}",
                        a.type_name(),
                        b.type_name()
                    ),
                    at,
                });
            }
        };
        *a = result.map_err(|message| Fault {
            message: message.into(),
            at,
        })?;
        Ok(())
    }

    // The slot for a global name, made the first time the name is seen, in any program.
    fn global_slot(&mut self, name: &str) -> u16 {
        if let Some(&slot) = self.global_slots.get(name) {
            return slot;
        }
        let slot = self.globals.len() as u16;
        self.globals.push(None);
        self.global_names.push(name.to_string());
        self.global_slots.insert(name.to_string(), slot);
        slot
    }

    // Rewrites every global a function and the functions inside it use, from a name to a
    // slot, so running it never hashes a name. The slots outlive the program, which is what
    // lets a REPL line use a global an earlier line defined.
    fn link(&mut self, function: &Function) -> Rc<Function> {
        let mut chunk = function.chunk.clone();
        for i in 0..chunk.constants.len() {
            if let Value::Function(inner) = &chunk.constants[i] {
                let inner = Rc::clone(inner);
                chunk.constants[i] = Value::Function(self.link(&inner));
            }
        }
        for i in 0..chunk.code.len() {
            let slot_of = |vm: &mut Vm, k: u16| vm.global_slot(name_of(&chunk.constants, k));
            chunk.code[i] = match chunk.code[i] {
                Op::DefineGlobal(k) => Op::DefineGlobalAt(slot_of(self, k)),
                Op::GetGlobal(k) => Op::GetGlobalAt(slot_of(self, k)),
                Op::SetGlobal(k) => Op::SetGlobalAt(slot_of(self, k)),
                other => other,
            };
        }
        Rc::new(Function {
            name: function.name.clone(),
            arity: function.arity,
            chunk,
            upvalues: function.upvalues.clone(),
        })
    }

    pub fn run(&mut self, script: Rc<Function>) -> Result<Value, RuntimeError> {
        let script = self.link(&script);
        self.execute(script).map_err(|fault| {
            // The failing frame is the last one; the fault's index is into that function's code.
            let failing = &self
                .frames
                .last()
                .expect("a fault happens inside a frame")
                .function;
            let (at, source) = (failing.chunk.span(fault.at), failing.chunk.source);
            // The failing frame is at the fault itself; every frame below it is paused at its call.
            let mut trace = Vec::new();
            for (i, frame) in self.frames.iter().enumerate().rev() {
                let line = if i + 1 == self.frames.len() {
                    at.line
                } else {
                    frame.function.chunk.span(frame.ip - 1).line
                };
                trace.push((
                    frame.function.name.clone(),
                    frame.function.chunk.source,
                    line,
                ));
            }
            // Leave the machine ready for the next program, which is what a REPL will need.
            self.stack.clear();
            self.frames.clear();
            self.open_upvalues.clear();
            RuntimeError {
                message: fault.message,
                at,
                source,
                trace,
            }
        })
    }

    // The upvalue for an absolute stack slot, shared with any closure that already captured it.
    fn capture(&mut self, slot: usize) -> Ref {
        let heap = &self.heap;
        let existing = self
            .open_upvalues
            .iter()
            .find(|&&r| matches!(heap.upvalue(r), Upvalue::Open(s) if *s == slot));
        if let Some(&up) = existing {
            return up;
        }
        let up = self.heap.alloc(Obj::Upvalue(Upvalue::Open(slot)));
        self.open_upvalues.push(up);
        up
    }

    // Slots from `from` upwards are about to disappear; every upvalue pointing at one of
    // them takes its value off the stack and keeps it.
    fn close_from(&mut self, from: usize) {
        let heap = &mut self.heap;
        let stack = &self.stack;
        self.open_upvalues.retain(|&up| {
            let slot = match heap.upvalue(up) {
                Upvalue::Open(slot) if *slot >= from => *slot,
                _ => return true,
            };
            *heap.upvalue_mut(up) = Upvalue::Closed(stack[slot].clone());
            false
        });
    }

    // Runs only between instructions. By then every live value is on the stack, in a
    // global or in an open upvalue, so those are all the roots there are; no half-built
    // value can be sitting in a Rust variable where marking cannot see it. A running
    // frame's closure needs no root of its own: it sits in the frame's slot 0 on the stack.
    pub fn collect(&mut self) {
        let mut roots: Vec<Ref> = Vec::new();
        let handle = |v: &Value| match v {
            Value::List(r) | Value::Map(r) | Value::Closure(r) => Some(*r),
            _ => None,
        };
        roots.extend(self.stack.iter().filter_map(handle));
        roots.extend(self.globals.iter().flatten().filter_map(handle));
        roots.extend(self.open_upvalues.iter().copied());
        self.heap.mark(roots);
        self.heap.sweep();
    }

    fn new_list(&mut self, items: Vec<Value>) -> Value {
        Value::List(self.heap.alloc(Obj::List(items)))
    }

    fn execute(&mut self, script: Rc<Function>) -> Result<Value, Fault> {
        let closure = self.heap.alloc(Obj::Closure(Closure {
            function: Rc::clone(&script),
            upvalues: Vec::new(),
        }));
        self.stack.push(Value::Closure(closure));
        self.frames.push(Frame {
            closure,
            function: Rc::clone(&script),
            ip: 0,
            base: 0,
        });
        // The running frame's state is kept in locals and written back only on a call.
        let mut closure = closure;
        let mut func = script;
        let mut ip = 0;
        let mut base = 0;
        loop {
            if self.heap.wants_collection() {
                self.collect();
            }
            let chunk = &func.chunk;
            let op = chunk.code[ip];
            let at = ip;
            ip += 1;
            match op {
                Op::Constant(k) => self.stack.push(chunk.constants[k as usize].clone()),
                Op::Nil => self.stack.push(Value::Nil),
                Op::True => self.stack.push(Value::Bool(true)),
                Op::False => self.stack.push(Value::Bool(false)),
                // Two numbers, the common case, are added in place like the other operators.
                Op::Add
                    if matches!(
                        self.stack.as_slice(),
                        [.., Value::Number(_), Value::Number(_)]
                    ) =>
                {
                    self.arith("+", at, |x, y| Ok(Value::Number(x + y)))?
                }
                Op::Add => {
                    let b = self.pop();
                    let a = self.pop();
                    // + is the one operator that also means something for strings.
                    let result = match (a, b) {
                        (Value::Number(x), Value::Number(y)) => Value::Number(x + y),
                        (Value::Str(x), Value::Str(y)) => Value::Str(x + &y),
                        // Joining makes a new list; neither operand changes.
                        (Value::List(x), Value::List(y)) => {
                            let mut joined = self.heap.list(x).clone();
                            joined.extend(self.heap.list(y).iter().cloned());
                            self.new_list(joined)
                        }
                        (a, b) => {
                            return Err(Fault {
                                message: format!(
                                    "+ needs two numbers, two strings or two lists, not a {} and a {}",
                                    a.type_name(),
                                    b.type_name()
                                ),
                                at,
                            });
                        }
                    };
                    self.stack.push(result);
                }
                Op::Sub => self.arith("-", at, |x, y| Ok(Value::Number(x - y)))?,
                Op::Mul => self.arith("*", at, |x, y| Ok(Value::Number(x * y)))?,
                // Dividing by zero is an error rather than infinity, which is what people expect.
                Op::Div => self.arith("/", at, |x, y| {
                    if y == 0.0 {
                        Err("division by zero")
                    } else {
                        Ok(Value::Number(x / y))
                    }
                })?,
                Op::Rem => self.arith("%", at, |x, y| {
                    if y == 0.0 {
                        Err("remainder by zero")
                    } else {
                        Ok(Value::Number(x % y))
                    }
                })?,
                Op::Neg => match self.pop() {
                    Value::Number(n) => self.stack.push(Value::Number(-n)),
                    other => {
                        return Err(Fault {
                            message: format!("- needs a number, not a {}", other.type_name()),
                            at,
                        });
                    }
                },
                Op::Not => {
                    let v = self.pop();
                    self.stack.push(Value::Bool(!v.truthy()));
                }
                Op::Eq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(a == b));
                }
                Op::NotEq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(a != b));
                }
                Op::Less => self.compare("<", at, Ordering::is_lt)?,
                Op::LessEq => self.compare("<=", at, Ordering::is_le)?,
                Op::Greater => self.compare(">", at, Ordering::is_gt)?,
                Op::GreaterEq => self.compare(">=", at, Ordering::is_ge)?,
                Op::In => {
                    let container = self.pop();
                    let item = self.stack.last_mut().expect("a left operand");
                    let found = match (&container, &*item) {
                        (Value::List(r), _) => self.heap.list(*r).contains(item),
                        (Value::Map(r), Value::Str(key)) => self.heap.map(*r).contains_key(key),
                        (Value::Str(text), Value::Str(part)) => text.contains(part.as_str()),
                        (Value::Map(_), other) => {
                            return Err(Fault {
                                message: format!(
                                    "map keys must be strings, not a {}",
                                    other.type_name()
                                ),
                                at,
                            });
                        }
                        (Value::Str(_), other) => {
                            return Err(Fault {
                                message: format!(
                                    "in a string needs a string to look for, not a {}",
                                    other.type_name()
                                ),
                                at,
                            });
                        }
                        (other, _) => {
                            return Err(Fault {
                                message: format!(
                                    "in needs a list, a map or a string on its right, not a {}",
                                    other.type_name()
                                ),
                                at,
                            });
                        }
                    };
                    *item = Value::Bool(found);
                }
                Op::Pop => {
                    self.pop();
                }
                Op::Return => {
                    let result = self.pop();
                    let finished = self.frames.pop().expect("a frame to return from");
                    // The callee, its arguments and its locals all go at once, after anything
                    // captured from them has been moved off the stack.
                    self.close_from(finished.base);
                    self.stack.truncate(finished.base);
                    let Some(caller) = self.frames.last() else {
                        return Ok(result);
                    };
                    closure = caller.closure;
                    func = Rc::clone(&caller.function);
                    ip = caller.ip;
                    base = caller.base;
                    self.stack.push(result);
                }
                Op::Call(argc) => {
                    let callee_at = self.stack.len() - 1 - argc as usize;
                    let callee = match &self.stack[callee_at] {
                        Value::Closure(c) => *c,
                        Value::Native(native) => {
                            let native = Rc::clone(native);
                            if native.arity.is_some_and(|n| n != argc) {
                                return Err(Fault {
                                    message: format!(
                                        "{} takes {} arguments, but was given {argc}",
                                        native.name,
                                        native.arity.unwrap_or(0)
                                    ),
                                    at,
                                });
                            }
                            // A native needs no frame: it runs to completion right here.
                            let args = self.stack.split_off(callee_at + 1);
                            self.stack.pop();
                            let result = (native.call)(self, &args)
                                .map_err(|message| Fault { message, at })?;
                            self.stack.push(result);
                            continue;
                        }
                        other => {
                            return Err(Fault {
                                message: format!("a {} cannot be called", other.type_name()),
                                at,
                            });
                        }
                    };
                    let callee_func = Rc::clone(&self.heap.closure(callee).function);
                    if callee_func.arity != argc {
                        return Err(Fault {
                            message: format!(
                                "{} takes {} argument{}, but was given {argc}",
                                callee_func.name,
                                callee_func.arity,
                                if callee_func.arity == 1 { "" } else { "s" }
                            ),
                            at,
                        });
                    }
                    if self.frames.len() >= MAX_FRAMES {
                        return Err(Fault {
                            message: format!(
                                "stack overflow: more than {MAX_FRAMES} calls deep, in {}",
                                callee_func.name
                            ),
                            at,
                        });
                    }
                    self.frames.last_mut().expect("a caller").ip = ip;
                    self.frames.push(Frame {
                        closure: callee,
                        function: Rc::clone(&callee_func),
                        ip: 0,
                        base: callee_at,
                    });
                    closure = callee;
                    func = callee_func;
                    ip = 0;
                    base = callee_at;
                }
                Op::DefineGlobal(_) | Op::GetGlobal(_) | Op::SetGlobal(_) => {
                    unreachable!("link() replaces every named global before a program runs")
                }
                Op::DefineGlobalAt(slot) => {
                    self.globals[slot as usize] = Some(self.pop());
                }
                Op::GetGlobalAt(slot) => match &self.globals[slot as usize] {
                    Some(v) => self.stack.push(v.clone()),
                    None => {
                        return Err(Fault {
                            message: format!("{} is not defined", self.global_names[slot as usize]),
                            at,
                        });
                    }
                },
                Op::SetGlobalAt(slot) => {
                    let value = self
                        .stack
                        .last()
                        .expect("assignment leaves its value")
                        .clone();
                    // Assigning never creates a variable, so a typo is an error rather than a new global.
                    match &mut self.globals[slot as usize] {
                        Some(current) => *current = value,
                        None => {
                            return Err(Fault {
                                message: format!(
                                    "{} is not defined; declare it with let first",
                                    self.global_names[slot as usize]
                                ),
                                at,
                            });
                        }
                    }
                }
                Op::Jump(to) => ip = to as usize,
                Op::JumpIfFalse(to) => {
                    if !self.stack.last().expect("a condition to test").truthy() {
                        ip = to as usize;
                    }
                }
                Op::Closure(k) => {
                    let Value::Function(made) = &chunk.constants[k as usize] else {
                        unreachable!("a closure is made from a function constant");
                    };
                    let made = Rc::clone(made);
                    let upvalues = made
                        .upvalues
                        .iter()
                        .map(|r| {
                            if r.is_local {
                                self.capture(base + r.index as usize)
                            } else {
                                self.heap.closure(closure).upvalues[r.index as usize]
                            }
                        })
                        .collect();
                    let c = self.heap.alloc(Obj::Closure(Closure {
                        function: made,
                        upvalues,
                    }));
                    self.stack.push(Value::Closure(c));
                }
                Op::GetUpvalue(i) => {
                    let up = self.heap.closure(closure).upvalues[i as usize];
                    let value = match self.heap.upvalue(up) {
                        Upvalue::Open(slot) => self.stack[*slot].clone(),
                        Upvalue::Closed(v) => v.clone(),
                    };
                    self.stack.push(value);
                }
                Op::SetUpvalue(i) => {
                    let value = self
                        .stack
                        .last()
                        .expect("assignment leaves its value")
                        .clone();
                    let up = self.heap.closure(closure).upvalues[i as usize];
                    match self.heap.upvalue_mut(up) {
                        Upvalue::Open(slot) => self.stack[*slot] = value,
                        Upvalue::Closed(v) => *v = value,
                    }
                }
                Op::CloseUpvalue => {
                    self.close_from(self.stack.len() - 1);
                    self.pop();
                }
                Op::BuildList(n) => {
                    let items = self.stack.split_off(self.stack.len() - n as usize);
                    let list = self.new_list(items);
                    self.stack.push(list);
                }
                Op::BuildMap(n) => {
                    let flat = self.stack.split_off(self.stack.len() - 2 * n as usize);
                    let mut entries = BTreeMap::new();
                    for pair in flat.chunks(2) {
                        entries.insert(map_key(&pair[0], at)?, pair[1].clone());
                    }
                    let map = self.heap.alloc(Obj::Map(entries));
                    self.stack.push(Value::Map(map));
                }
                Op::GetIndex => {
                    let index = self.pop();
                    let target = self.pop();
                    let value = match &target {
                        Value::List(r) => {
                            let items = self.heap.list(*r);
                            items[whole_index(&index, items.len(), "list", at)?].clone()
                        }
                        // By character, not byte, so "héllo"[1] is "é".
                        Value::Str(s) => {
                            let i = whole_index(&index, s.chars().count(), "string", at)?;
                            Value::Str(s.chars().nth(i).expect("checked above").to_string())
                        }
                        // A missing key is an error, as an undefined variable is; has() asks first.
                        Value::Map(r) => {
                            let key = map_key(&index, at)?;
                            match self.heap.map(*r).get(&key) {
                                Some(v) => v.clone(),
                                None => {
                                    return Err(Fault {
                                        message: format!("the map has no key {key:?}"),
                                        at,
                                    });
                                }
                            }
                        }
                        other => {
                            return Err(Fault {
                                message: format!("a {} cannot be indexed", other.type_name()),
                                at,
                            });
                        }
                    };
                    self.stack.push(value);
                }
                Op::SetIndex => {
                    let value = self.pop();
                    let index = self.pop();
                    let target = self.pop();
                    match &target {
                        Value::List(r) => {
                            let i = whole_index(&index, self.heap.list(*r).len(), "list", at)?;
                            self.heap.list_mut(*r)[i] = value.clone();
                        }
                        Value::Map(r) => {
                            let key = map_key(&index, at)?;
                            self.heap.map_mut(*r).insert(key, value.clone());
                        }
                        other => {
                            return Err(Fault {
                                message: format!(
                                    "only a list's or a map's items can be assigned, not a {}'s",
                                    other.type_name()
                                ),
                                at,
                            });
                        }
                    }
                    self.stack.push(value);
                }
                Op::Iterable => match self.stack.last().expect("a value to loop over") {
                    Value::List(_) | Value::Str(_) => {}
                    Value::Map(r) => {
                        let keys = self
                            .heap
                            .map(*r)
                            .keys()
                            .map(|k| Value::Str(k.clone()))
                            .collect();
                        let list = self.new_list(keys);
                        *self.stack.last_mut().expect("checked above") = list;
                    }
                    other => {
                        return Err(Fault {
                            message: format!(
                                "for needs a list, a string or a map, not a {}",
                                other.type_name()
                            ),
                            at,
                        });
                    }
                },
                Op::Len => {
                    let n = match self.pop() {
                        Value::List(r) => self.heap.list(r).len(),
                        Value::Str(s) => s.chars().count(),
                        other => unreachable!(
                            "Iterable lets only a list or a string through, not {other}"
                        ),
                    };
                    self.stack.push(Value::Number(n as f64));
                }
                Op::GetLocal(slot) => self.stack.push(self.stack[base + slot as usize].clone()),
                Op::SetLocal(slot) => {
                    let value = self
                        .stack
                        .last()
                        .expect("assignment leaves its value")
                        .clone();
                    self.stack[base + slot as usize] = value;
                }
            }
        }
    }
}

// An index must be a whole number. A negative one counts from the end, as in Python, so
// -1 is the last item; from -len to len - 1 is in range.
fn whole_index(index: &Value, len: usize, what: &str, at: usize) -> Result<usize, Fault> {
    let n = match index {
        Value::Number(n) if n.fract() == 0.0 => *n,
        other => {
            return Err(Fault {
                message: format!("a {what} index must be a whole number, not {other}"),
                at,
            });
        }
    };
    let from_start = if n < 0.0 { n + len as f64 } else { n };
    if from_start < 0.0 || from_start >= len as f64 {
        return Err(Fault {
            message: format!("index {n} is out of range for a {what} of length {len}"),
            at,
        });
    }
    Ok(from_start as usize)
}

fn map_key(key: &Value, at: usize) -> Result<String, Fault> {
    match key {
        Value::Str(s) => Ok(s.clone()),
        other => Err(Fault {
            message: format!("map keys must be strings, not a {}", other.type_name()),
            at,
        }),
    }
}

fn name_of(constants: &[Value], k: u16) -> &str {
    match &constants[k as usize] {
        Value::Str(s) => s,
        other => unreachable!("a global's name constant is a string, not {other}"),
    }
}

const NATIVES: [Native; 18] = [
    Native {
        name: "print",
        arity: None,
        call: native_print,
    },
    Native {
        name: "clock",
        arity: Some(0),
        call: native_clock,
    },
    Native {
        name: "len",
        arity: Some(1),
        call: native_len,
    },
    Native {
        name: "push",
        arity: Some(2),
        call: native_push,
    },
    Native {
        name: "keys",
        arity: Some(1),
        call: native_keys,
    },
    Native {
        name: "has",
        arity: Some(2),
        call: native_has,
    },
    Native {
        name: "str",
        arity: Some(1),
        call: native_str,
    },
    Native {
        name: "num",
        arity: Some(1),
        call: native_num,
    },
    Native {
        name: "range",
        arity: None,
        call: native_range,
    },
    Native {
        name: "pop",
        arity: Some(1),
        call: native_pop,
    },
    Native {
        name: "join",
        arity: Some(2),
        call: native_join,
    },
    Native {
        name: "split",
        arity: Some(2),
        call: native_split,
    },
    Native {
        name: "floor",
        arity: Some(1),
        call: native_floor,
    },
    Native {
        name: "sort",
        arity: Some(1),
        call: native_sort,
    },
    Native {
        name: "slice",
        arity: None,
        call: native_slice,
    },
    Native {
        name: "find",
        arity: Some(2),
        call: native_find,
    },
    Native {
        name: "upper",
        arity: Some(1),
        call: native_upper,
    },
    Native {
        name: "lower",
        arity: Some(1),
        call: native_lower,
    },
];

// Where part first occurs in s, counted in characters like indexing and slice, so the
// answer can be passed straight back to them; -1 if it does not occur.
fn native_find(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match (&args[0], &args[1]) {
        (Value::Str(s), Value::Str(part)) => Ok(Value::Number(match s.find(part.as_str()) {
            Some(byte) => s[..byte].chars().count() as f64,
            None => -1.0,
        })),
        (Value::Str(_), other) => Err(format!(
            "find needs a string to look for, not a {}",
            other.type_name()
        )),
        (other, _) => Err(format!("find needs a string, not a {}", other.type_name())),
    }
}

fn native_upper(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::Str(s) => Ok(Value::Str(s.to_uppercase())),
        other => Err(format!("upper needs a string, not a {}", other.type_name())),
    }
}

fn native_lower(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::Str(s) => Ok(Value::Str(s.to_lowercase())),
        other => Err(format!("lower needs a string, not a {}", other.type_name())),
    }
}

// slice(xs, start) or slice(xs, start, end): a new list or string, the end not included.
// Negative positions count from the end, and unlike an index a slice is trimmed to fit
// rather than an error, as in Python, so "the first ten" works on a shorter list.
fn native_slice(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    let (start, end) = match args {
        [_, start] => (start, None),
        [_, start, end] => (start, Some(end)),
        _ => {
            return Err(format!(
                "slice takes 2 or 3 arguments, but was given {}",
                args.len()
            ));
        }
    };
    let whole = |v: &Value| match v {
        Value::Number(n) if n.fract() == 0.0 => Ok(*n),
        other => Err(format!("slice needs whole numbers, not {other}")),
    };
    let (start, end) = (whole(start)?, end.map(whole).transpose()?);
    let range = |len: usize| {
        let fit =
            |n: f64| (if n < 0.0 { n + len as f64 } else { n }).clamp(0.0, len as f64) as usize;
        let from = fit(start);
        (from, end.map_or(len, fit).max(from))
    };
    match &args[0] {
        Value::List(r) => {
            let items = vm.heap.list(*r);
            let (from, to) = range(items.len());
            let part = items[from..to].to_vec();
            Ok(vm.new_list(part))
        }
        // By character, as indexing is, so "héllo" slices around the é, not through it.
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let (from, to) = range(chars.len());
            Ok(Value::Str(chars[from..to].iter().collect()))
        }
        other => Err(format!(
            "slice needs a list or a string, not a {}",
            other.type_name()
        )),
    }
}

// Sorts the list itself, as push changes it: numbers ascending, or strings by code point,
// the order < uses. A mix is an error rather than a guess at which order was meant.
fn native_sort(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    let Value::List(r) = &args[0] else {
        return Err(format!("sort needs a list, not a {}", args[0].type_name()));
    };
    let items = vm.heap.list_mut(*r);
    if items.iter().all(|v| matches!(v, Value::Number(_))) {
        // A total order, so even a NaN has a place and sorting cannot panic.
        items.sort_by(|a, b| match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.total_cmp(y),
            _ => unreachable!("checked above"),
        });
    } else if items.iter().all(|v| matches!(v, Value::Str(_))) {
        items.sort_by(|a, b| match (a, b) {
            (Value::Str(x), Value::Str(y)) => x.cmp(y),
            _ => unreachable!("checked above"),
        });
    } else {
        return Err("sort needs a list of all numbers or all strings".into());
    }
    Ok(Value::Nil)
}

// Any value as text, exactly as print would show it.
fn native_str(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(vm.heap.show(&args[0])))
}

fn native_num(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::Str(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(Value::Number)
            .ok_or_else(|| format!("num could not read {s:?} as a number")),
        other => Err(format!("num needs a string, not a {}", other.type_name())),
    }
}

// range(n) is 0 up to n; range(a, b) is a up to b. The end is never included, as in Python.
fn native_range(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    let whole = |v: &Value| match v {
        Value::Number(n) if n.fract() == 0.0 => Ok(*n as i64),
        other => Err(format!(
            "range needs whole numbers, not {}",
            vm.heap.show(other)
        )),
    };
    let (from, to) = match args {
        [end] => (0, whole(end)?),
        [start, end] => (whole(start)?, whole(end)?),
        _ => {
            return Err(format!(
                "range takes 1 or 2 arguments, but was given {}",
                args.len()
            ));
        }
    };
    let items = (from..to.max(from))
        .map(|i| Value::Number(i as f64))
        .collect();
    Ok(vm.new_list(items))
}

// Takes the last item off the list itself and returns it, so a list works as a stack.
fn native_pop(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::List(r) => vm
            .heap
            .list_mut(*r)
            .pop()
            .ok_or_else(|| "pop needs a list with something in it".to_string()),
        other => Err(format!("pop needs a list, not a {}", other.type_name())),
    }
}

// Items are shown as print shows them, so a list of numbers joins without converting first.
fn native_join(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match (&args[0], &args[1]) {
        (Value::List(r), Value::Str(sep)) => Ok(Value::Str(
            vm.heap
                .list(*r)
                .iter()
                .map(|v| vm.heap.show(v))
                .collect::<Vec<_>>()
                .join(sep),
        )),
        (Value::List(_), other) => Err(format!(
            "join needs a string to put between items, not a {}",
            other.type_name()
        )),
        (other, _) => Err(format!("join needs a list, not a {}", other.type_name())),
    }
}

// An empty separator splits into characters.
fn native_split(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match (&args[0], &args[1]) {
        (Value::Str(s), Value::Str(sep)) => {
            let parts: Vec<Value> = if sep.is_empty() {
                s.chars().map(|c| Value::Str(c.to_string())).collect()
            } else {
                s.split(sep.as_str())
                    .map(|p| Value::Str(p.to_string()))
                    .collect()
            };
            Ok(vm.new_list(parts))
        }
        (Value::Str(_), other) => Err(format!(
            "split needs a string to split on, not a {}",
            other.type_name()
        )),
        (other, _) => Err(format!("split needs a string, not a {}", other.type_name())),
    }
}

// Every number is a float, so whole-number division is floor(a / b).
fn native_floor(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::Number(n) => Ok(Value::Number(n.floor())),
        other => Err(format!("floor needs a number, not a {}", other.type_name())),
    }
}

// A map's keys as a new list, in the same sorted order the map prints in.
fn native_keys(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::Map(r) => {
            let keys = vm
                .heap
                .map(*r)
                .keys()
                .map(|k| Value::Str(k.clone()))
                .collect();
            Ok(vm.new_list(keys))
        }
        other => Err(format!("keys needs a map, not a {}", other.type_name())),
    }
}

fn native_has(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match (&args[0], &args[1]) {
        (Value::Map(r), Value::Str(key)) => Ok(Value::Bool(vm.heap.map(*r).contains_key(key))),
        (Value::Map(_), other) => Err(format!(
            "map keys must be strings, not a {}",
            other.type_name()
        )),
        (other, _) => Err(format!("has needs a map, not a {}", other.type_name())),
    }
}

fn native_len(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::List(r) => Ok(Value::Number(vm.heap.list(*r).len() as f64)),
        Value::Str(s) => Ok(Value::Number(s.chars().count() as f64)),
        Value::Map(r) => Ok(Value::Number(vm.heap.map(*r).len() as f64)),
        other => Err(format!(
            "len needs a list, a string or a map, not a {}",
            other.type_name()
        )),
    }
}

// Adds to the end of the list itself, so every name for that list sees it.
fn native_push(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::List(r) => {
            vm.heap.list_mut(*r).push(args[1].clone());
            Ok(Value::Nil)
        }
        other => Err(format!("push needs a list, not a {}", other.type_name())),
    }
}

// Prints its arguments separated by spaces, as Python does.
fn native_print(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    let line = args
        .iter()
        .map(|v| vm.heap.show(v))
        .collect::<Vec<_>>()
        .join(" ");
    if vm.echo {
        println!("{line}");
    }
    vm.output.push(line);
    Ok(Value::Nil)
}

// Seconds since the program's first call to it, for timing code from inside the language.
fn native_clock(_vm: &mut Vm, _args: &[Value]) -> Result<Value, String> {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    Ok(Value::Number(
        START.get_or_init(Instant::now).elapsed().as_secs_f64(),
    ))
}
