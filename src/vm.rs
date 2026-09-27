use crate::chunk::{Chunk, Function, Native, Op, Value};
use crate::heap::{Closure, Heap, Obj, Ref, Upvalue};
use crate::lexer::Span;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

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

// What the run loop raises: what went wrong and where. run() adds the call stack.
struct Fault {
    message: String,
    at: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    pub at: Span,
    // Innermost call first: the function's name and the line it had reached.
    pub trace: Vec<(String, u32)>,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.message, self.at)?;
        // Runaway recursion is ten thousand identical frames; say so once, as Python does.
        let mut i = 0;
        while i < self.trace.len() {
            let (name, line) = &self.trace[i];
            let repeats = self.trace[i..]
                .iter()
                .take_while(|entry| *entry == &self.trace[i])
                .count();
            write!(f, "\n  in {name}, line {line}")?;
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
    globals: HashMap<String, Value>,
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
            globals: HashMap::new(),
            open_upvalues: Vec::new(),
            heap: Heap::default(),
            output: Vec::new(),
            echo: false,
        };
        for native in NATIVES {
            vm.globals
                .insert(native.name.to_string(), Value::Native(Rc::new(native)));
        }
        // EMBER_STRESS_GC=1 cargo test runs every test with a collection before every
        // instruction that follows an allocation.
        vm.heap.stress = std::env::var_os("EMBER_STRESS_GC").is_some();
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

    fn numbers(&mut self, op: &str, at: Span) -> Result<(f64, f64), Fault> {
        let b = self.pop();
        let a = self.pop();
        match (&a, &b) {
            (Value::Number(x), Value::Number(y)) => Ok((*x, *y)),
            _ => Err(Fault {
                message: format!(
                    "{op} needs two numbers, not a {} and a {}",
                    a.type_name(),
                    b.type_name()
                ),
                at,
            }),
        }
    }

    pub fn run(&mut self, script: Rc<Function>) -> Result<Value, RuntimeError> {
        self.execute(script).map_err(|fault| {
            // The failing frame is at the fault itself; every frame below it is paused at its call.
            let mut trace = Vec::new();
            for (i, frame) in self.frames.iter().enumerate().rev() {
                let line = if i + 1 == self.frames.len() {
                    fault.at.line
                } else {
                    frame.function.chunk.span(frame.ip - 1).line
                };
                trace.push((frame.function.name.clone(), line));
            }
            // Leave the machine ready for the next program, which is what a REPL will need.
            self.stack.clear();
            self.frames.clear();
            self.open_upvalues.clear();
            RuntimeError {
                message: fault.message,
                at: fault.at,
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
        roots.extend(self.globals.values().filter_map(handle));
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
            let at = chunk.span(ip);
            ip += 1;
            match op {
                Op::Constant(k) => self.stack.push(chunk.constants[k as usize].clone()),
                Op::Nil => self.stack.push(Value::Nil),
                Op::True => self.stack.push(Value::Bool(true)),
                Op::False => self.stack.push(Value::Bool(false)),
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
                Op::Sub => {
                    let (a, b) = self.numbers("-", at)?;
                    self.stack.push(Value::Number(a - b));
                }
                Op::Mul => {
                    let (a, b) = self.numbers("*", at)?;
                    self.stack.push(Value::Number(a * b));
                }
                Op::Div => {
                    let (a, b) = self.numbers("/", at)?;
                    // Dividing by zero is an error rather than infinity, which is what people expect.
                    if b == 0.0 {
                        return Err(Fault {
                            message: "division by zero".into(),
                            at,
                        });
                    }
                    self.stack.push(Value::Number(a / b));
                }
                Op::Rem => {
                    let (a, b) = self.numbers("%", at)?;
                    if b == 0.0 {
                        return Err(Fault {
                            message: "remainder by zero".into(),
                            at,
                        });
                    }
                    self.stack.push(Value::Number(a % b));
                }
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
                Op::Less => {
                    let (a, b) = self.numbers("<", at)?;
                    self.stack.push(Value::Bool(a < b));
                }
                Op::LessEq => {
                    let (a, b) = self.numbers("<=", at)?;
                    self.stack.push(Value::Bool(a <= b));
                }
                Op::Greater => {
                    let (a, b) = self.numbers(">", at)?;
                    self.stack.push(Value::Bool(a > b));
                }
                Op::GreaterEq => {
                    let (a, b) = self.numbers(">=", at)?;
                    self.stack.push(Value::Bool(a >= b));
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
                Op::DefineGlobal(k) => {
                    let name = name_of(chunk, k);
                    let value = self.pop();
                    self.globals.insert(name, value);
                }
                Op::GetGlobal(k) => {
                    let name = name_of(chunk, k);
                    match self.globals.get(&name) {
                        Some(v) => self.stack.push(v.clone()),
                        None => {
                            return Err(Fault {
                                message: format!("{name} is not defined"),
                                at,
                            });
                        }
                    }
                }
                Op::SetGlobal(k) => {
                    let name = name_of(chunk, k);
                    // Assigning never creates a variable, so a typo is an error rather than a new global.
                    if !self.globals.contains_key(&name) {
                        return Err(Fault {
                            message: format!("{name} is not defined; declare it with let first"),
                            at,
                        });
                    }
                    let value = self
                        .stack
                        .last()
                        .expect("assignment leaves its value")
                        .clone();
                    self.globals.insert(name, value);
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
                Op::Len => {
                    let n = match self.pop() {
                        Value::List(r) => self.heap.list(r).len(),
                        Value::Str(s) => s.chars().count(),
                        Value::Map(_) => {
                            return Err(Fault {
                                message: "for needs a list or a string, not a map; loop over keys(m) instead".into(),
                                at,
                            });
                        }
                        other => {
                            return Err(Fault {
                                message: format!(
                                    "for needs a list or a string, not a {}",
                                    other.type_name()
                                ),
                                at,
                            });
                        }
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

// An index must be a whole number inside the list; there is no negative indexing.
fn whole_index(index: &Value, len: usize, what: &str, at: Span) -> Result<usize, Fault> {
    let n = match index {
        Value::Number(n) if n.fract() == 0.0 => *n,
        other => {
            return Err(Fault {
                message: format!("a {what} index must be a whole number, not {other}"),
                at,
            });
        }
    };
    if n < 0.0 || n >= len as f64 {
        return Err(Fault {
            message: format!("index {n} is out of range for a {what} of length {len}"),
            at,
        });
    }
    Ok(n as usize)
}

fn map_key(key: &Value, at: Span) -> Result<String, Fault> {
    match key {
        Value::Str(s) => Ok(s.clone()),
        other => Err(Fault {
            message: format!("map keys must be strings, not a {}", other.type_name()),
            at,
        }),
    }
}

fn name_of(chunk: &Chunk, k: u16) -> String {
    match &chunk.constants[k as usize] {
        Value::Str(s) => s.clone(),
        other => unreachable!("a global's name constant is a string, not {other}"),
    }
}

const NATIVES: [Native; 6] = [
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
];

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
