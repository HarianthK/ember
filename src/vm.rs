use crate::chunk::{Chunk, Closure, Function, Native, Op, Upvalue, Value};
use crate::lexer::Span;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

// Deep enough for any honest recursion, shallow enough to stop a runaway one quickly.
const MAX_FRAMES: usize = 10_000;

// A function call in progress. `base` is where its slot 0 sits on the shared stack.
struct Frame {
    closure: Rc<Closure>,
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
    open_upvalues: Vec<Rc<RefCell<Upvalue>>>,
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
            output: Vec::new(),
            echo: false,
        };
        for native in NATIVES {
            vm.globals
                .insert(native.name.to_string(), Value::Native(Rc::new(native)));
        }
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
                    frame.closure.function.chunk.span(frame.ip - 1).line
                };
                trace.push((frame.closure.function.name.clone(), line));
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
    fn capture(&mut self, slot: usize) -> Rc<RefCell<Upvalue>> {
        let existing = self
            .open_upvalues
            .iter()
            .find(|u| matches!(*u.borrow(), Upvalue::Open(s) if s == slot));
        if let Some(up) = existing {
            return Rc::clone(up);
        }
        let up = Rc::new(RefCell::new(Upvalue::Open(slot)));
        self.open_upvalues.push(Rc::clone(&up));
        up
    }

    // Slots from `from` upwards are about to disappear; every upvalue pointing at one of
    // them takes its value off the stack and keeps it.
    fn close_from(&mut self, from: usize) {
        let stack = &self.stack;
        self.open_upvalues.retain(|up| {
            let slot = match *up.borrow() {
                Upvalue::Open(slot) if slot >= from => slot,
                _ => return true,
            };
            *up.borrow_mut() = Upvalue::Closed(stack[slot].clone());
            false
        });
    }

    fn execute(&mut self, script: Rc<Function>) -> Result<Value, Fault> {
        let script = Rc::new(Closure {
            function: script,
            upvalues: Vec::new(),
        });
        self.stack.push(Value::Closure(Rc::clone(&script)));
        self.frames.push(Frame {
            closure: Rc::clone(&script),
            ip: 0,
            base: 0,
        });
        // The running frame's state is kept in locals and written back only on a call.
        let mut closure = script;
        let mut ip = 0;
        let mut base = 0;
        loop {
            let chunk = &closure.function.chunk;
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
                            let mut joined = x.borrow().clone();
                            joined.extend(y.borrow().iter().cloned());
                            Value::List(Rc::new(RefCell::new(joined)))
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
                    closure = Rc::clone(&caller.closure);
                    ip = caller.ip;
                    base = caller.base;
                    self.stack.push(result);
                }
                Op::Call(argc) => {
                    let callee_at = self.stack.len() - 1 - argc as usize;
                    let callee = match &self.stack[callee_at] {
                        Value::Closure(c) => Rc::clone(c),
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
                    let func = &callee.function;
                    if func.arity != argc {
                        return Err(Fault {
                            message: format!(
                                "{} takes {} argument{}, but was given {argc}",
                                func.name,
                                func.arity,
                                if func.arity == 1 { "" } else { "s" }
                            ),
                            at,
                        });
                    }
                    if self.frames.len() >= MAX_FRAMES {
                        return Err(Fault {
                            message: format!(
                                "stack overflow: more than {MAX_FRAMES} calls deep, in {}",
                                func.name
                            ),
                            at,
                        });
                    }
                    self.frames.last_mut().expect("a caller").ip = ip;
                    self.frames.push(Frame {
                        closure: Rc::clone(&callee),
                        ip: 0,
                        base: callee_at,
                    });
                    closure = callee;
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
                    let Value::Function(func) = &chunk.constants[k as usize] else {
                        unreachable!("a closure is made from a function constant");
                    };
                    let func = Rc::clone(func);
                    let upvalues = func
                        .upvalues
                        .iter()
                        .map(|r| {
                            if r.is_local {
                                self.capture(base + r.index as usize)
                            } else {
                                Rc::clone(&closure.upvalues[r.index as usize])
                            }
                        })
                        .collect();
                    self.stack.push(Value::Closure(Rc::new(Closure {
                        function: func,
                        upvalues,
                    })));
                }
                Op::GetUpvalue(i) => {
                    let value = match &*closure.upvalues[i as usize].borrow() {
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
                    match &mut *closure.upvalues[i as usize].borrow_mut() {
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
                    self.stack.push(Value::List(Rc::new(RefCell::new(items))));
                }
                Op::BuildMap(n) => {
                    let flat = self.stack.split_off(self.stack.len() - 2 * n as usize);
                    let mut entries = BTreeMap::new();
                    for pair in flat.chunks(2) {
                        entries.insert(map_key(&pair[0], at)?, pair[1].clone());
                    }
                    self.stack.push(Value::Map(Rc::new(RefCell::new(entries))));
                }
                Op::GetIndex => {
                    let index = self.pop();
                    let target = self.pop();
                    let value = match &target {
                        Value::List(items) => {
                            let items = items.borrow();
                            items[whole_index(&index, items.len(), "list", at)?].clone()
                        }
                        // By character, not byte, so "héllo"[1] is "é".
                        Value::Str(s) => {
                            let i = whole_index(&index, s.chars().count(), "string", at)?;
                            Value::Str(s.chars().nth(i).expect("checked above").to_string())
                        }
                        // A missing key is an error, as an undefined variable is; has() asks first.
                        Value::Map(entries) => {
                            let key = map_key(&index, at)?;
                            match entries.borrow().get(&key) {
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
                        Value::List(items) => {
                            let i = whole_index(&index, items.borrow().len(), "list", at)?;
                            items.borrow_mut()[i] = value.clone();
                        }
                        Value::Map(entries) => {
                            let key = map_key(&index, at)?;
                            entries.borrow_mut().insert(key, value.clone());
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
                        Value::List(items) => items.borrow().len(),
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
fn native_keys(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::Map(entries) => Ok(Value::List(Rc::new(RefCell::new(
            entries
                .borrow()
                .keys()
                .map(|k| Value::Str(k.clone()))
                .collect(),
        )))),
        other => Err(format!("keys needs a map, not a {}", other.type_name())),
    }
}

fn native_has(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match (&args[0], &args[1]) {
        (Value::Map(entries), Value::Str(key)) => {
            Ok(Value::Bool(entries.borrow().contains_key(key)))
        }
        (Value::Map(_), other) => Err(format!(
            "map keys must be strings, not a {}",
            other.type_name()
        )),
        (other, _) => Err(format!("has needs a map, not a {}", other.type_name())),
    }
}

fn native_len(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::List(items) => Ok(Value::Number(items.borrow().len() as f64)),
        Value::Str(s) => Ok(Value::Number(s.chars().count() as f64)),
        Value::Map(entries) => Ok(Value::Number(entries.borrow().len() as f64)),
        other => Err(format!(
            "len needs a list, a string or a map, not a {}",
            other.type_name()
        )),
    }
}

// Adds to the end of the list itself, so every name for that list sees it.
fn native_push(_vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::List(items) => {
            items.borrow_mut().push(args[1].clone());
            Ok(Value::Nil)
        }
        other => Err(format!("push needs a list, not a {}", other.type_name())),
    }
}

// Prints its arguments separated by spaces, as Python does.
fn native_print(vm: &mut Vm, args: &[Value]) -> Result<Value, String> {
    let line = args
        .iter()
        .map(|v| v.to_string())
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
