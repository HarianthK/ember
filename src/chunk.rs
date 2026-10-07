use crate::heap::Ref;
use crate::lexer::Span;
use crate::vm::Vm;
use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    Nil,
    // A compiled function as it sits in the constant table; only a closure is ever called.
    Function(Rc<Function>),
    Native(Rc<Native>),
    // Objects the collector manages. The value is a handle into the VM's heap, so
    // `let b = a` makes b the same list, as in Python.
    Closure(Ref),
    List(Ref),
    Map(Ref),
}

// A function written in Rust. `arity` of None takes any number of arguments.
pub struct Native {
    pub name: &'static str,
    pub arity: Option<u8>,
    pub call: fn(&mut Vm, &[Value]) -> Result<Value, String>,
}

impl fmt::Debug for Native {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<native {}>", self.name)
    }
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub arity: u8,
    pub chunk: Chunk,
    // What a closure of this function captures, in order, when it is created.
    pub upvalues: Vec<UpvalueRef>,
}

// Where to find a captured variable when the closure is made: a slot of the function
// creating it, or one of that function's own upvalues, for variables from further out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpvalueRef {
    pub is_local: bool,
    pub index: u16,
}

// Functions, lists and maps are equal only to themselves. Lists compare by identity, as in
// Lua and JavaScript, because a list can contain itself and comparing contents would not end.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Nil, Value::Nil) => true,
            (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
            (Value::Native(a), Value::Native(b)) => Rc::ptr_eq(a, b),
            (Value::Closure(a), Value::Closure(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    // Only nil and false are false; 0 and "" are true, as in Lua rather than C.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::Str(_) => "string",
            Value::Bool(_) => "boolean",
            Value::Nil => "nil",
            Value::Function(_) | Value::Closure(_) | Value::Native(_) => "function",
            Value::List(_) => "list",
            Value::Map(_) => "map",
        }
    }
}

// Without the heap there is no way to see inside a list; Heap::show prints the contents.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Nil => write!(f, "nil"),
            Value::Function(func) => write!(f, "<fn {}>", func.name),
            Value::Native(native) => write!(f, "<native {}>", native.name),
            Value::Closure(_) => write!(f, "<fn>"),
            Value::List(_) => write!(f, "<list>"),
            Value::Map(_) => write!(f, "<map>"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    // Operand is an index into the constant table.
    Constant(u16),
    Nil,
    True,
    False,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,
    Not,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    // Whether the value below is in the list, among the map's keys, or inside the string on top.
    In,
    Pop,
    Return,
    // Globals as the compiler emits them, by name: the operand is the name's constant.
    DefineGlobal(u16),
    GetGlobal(u16),
    SetGlobal(u16),
    // The same after linking, which gives every global name a numbered slot in the VM.
    // The VM only ever runs these; the named forms are replaced before the first instruction.
    DefineGlobalAt(u16),
    GetGlobalAt(u16),
    SetGlobalAt(u16),
    // Locals live on the stack; the operand is the slot, so no name is looked up at run time.
    GetLocal(u16),
    SetLocal(u16),
    // The operand is the argument count; the function sits on the stack just below the arguments.
    Call(u8),
    // Makes a closure from the function constant, capturing what its UpvalueRefs name.
    Closure(u16),
    GetUpvalue(u16),
    SetUpvalue(u16),
    // Ends a local that a closure captured: its value moves off the stack into the upvalue.
    CloseUpvalue,
    // Takes that many values off the stack, in order, into a new list.
    BuildList(u16),
    // Takes that many key and value pairs off the stack into a new map.
    BuildMap(u16),
    GetIndex,
    SetIndex,
    // Starts a for loop: a map becomes a list of its keys, taken once, so changing the map
    // inside the loop cannot change what it walks; a list or string is left as it is.
    Iterable,
    // The length of the list or string on top of the stack, for `for` to count with.
    Len,
    // Jumps name the instruction to go to, not a distance, so one op serves both directions.
    Jump(u16),
    // Leaves the condition on the stack; the code on each side pops it.
    JumpIfFalse(u16),
}

// Bytecode plus the line each instruction came from, kept beside it rather than
// inside it so the run loop never walks over the debug information.
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub spans: Vec<Span>,
    pub constants: Vec<Value>,
    // Where the code came from, for errors: empty for the program being run, "prelude" for
    // the library every VM loads, so a line number is never mistaken for one in the program.
    pub source: &'static str,
    // For each global lookup made where locals were in scope, its index and their names, so
    // "did you mean" can offer a local; only read when the lookup fails.
    pub nearby: Vec<(usize, Vec<String>)>,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk::default()
    }

    pub fn push(&mut self, op: Op, at: Span) -> usize {
        self.code.push(op);
        self.spans.push(at);
        self.code.len() - 1
    }

    pub fn nearby_at(&self, ip: usize) -> &[String] {
        match self.nearby.binary_search_by_key(&ip, |(i, _)| *i) {
            Ok(k) => &self.nearby[k].1,
            Err(_) => &[],
        }
    }

    // Constants are de-duplicated, so a loop mentioning the same number twice stores one.
    pub fn constant(&mut self, value: Value) -> u16 {
        if let Some(i) = self.constants.iter().position(|v| v == &value) {
            return i as u16;
        }
        self.constants.push(value);
        (self.constants.len() - 1) as u16
    }

    pub fn span(&self, ip: usize) -> Span {
        self.spans
            .get(ip)
            .copied()
            .unwrap_or(Span { line: 0, col: 0 })
    }

    pub fn disassemble(&self) -> String {
        let mut out = String::new();
        let mut last_line = 0;
        for (i, op) in self.code.iter().enumerate() {
            let line = self.spans[i].line;
            // A run of instructions from one line shows the line once, as disassemblers do.
            let line_text = if line == last_line {
                "   |".to_string()
            } else {
                format!("{line:>4}")
            };
            last_line = line;
            let text = match op {
                Op::DefineGlobal(k) | Op::GetGlobal(k) | Op::SetGlobal(k) => {
                    let name = format!("{op:?}");
                    let name = name[..name.find('(').unwrap()].to_uppercase();
                    format!("{name:<12} {k:>4} ({})", self.constants[*k as usize])
                }
                Op::DefineGlobalAt(slot) | Op::GetGlobalAt(slot) | Op::SetGlobalAt(slot) => {
                    let name = format!("{op:?}");
                    let name = name[..name.find('(').unwrap()].to_uppercase();
                    format!("{name:<12} {slot:>4}")
                }
                Op::Call(argc) => format!("{:<12} {argc:>4}", "CALL"),
                Op::BuildList(n) => format!("{:<12} {n:>4}", "BUILDLIST"),
                Op::BuildMap(n) => format!("{:<12} {n:>4}", "BUILDMAP"),
                Op::Closure(k) => {
                    let captures = match &self.constants[*k as usize] {
                        Value::Function(func) => func
                            .upvalues
                            .iter()
                            .map(|u| {
                                format!(
                                    "{} {}",
                                    if u.is_local { "local" } else { "upvalue" },
                                    u.index
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", "),
                        _ => String::new(),
                    };
                    format!(
                        "{:<12} {k:>4} ({}) [{captures}]",
                        "CLOSURE", self.constants[*k as usize]
                    )
                }
                Op::GetUpvalue(i) | Op::SetUpvalue(i) => {
                    let name = format!("{op:?}");
                    let name = name[..name.find('(').unwrap()].to_uppercase();
                    format!("{name:<12} {i:>4}")
                }
                Op::Jump(to) | Op::JumpIfFalse(to) => {
                    let name = format!("{op:?}");
                    let name = name[..name.find('(').unwrap()].to_uppercase();
                    format!("{name:<12} -> {to:04}")
                }
                Op::GetLocal(slot) | Op::SetLocal(slot) => {
                    let name = format!("{op:?}");
                    let name = name[..name.find('(').unwrap()].to_uppercase();
                    format!("{name:<12} {slot:>4}")
                }
                Op::Constant(k) => {
                    format!(
                        "{:<12} {k:>4} ({})",
                        "CONSTANT", self.constants[*k as usize]
                    )
                }
                other => format!("{other:?}").to_uppercase(),
            };
            out.push_str(&format!("{i:04} {line_text} {text}\n"));
        }
        // A function's body is its own chunk, stored as a constant; list each one after.
        for value in &self.constants {
            if let Value::Function(func) = value {
                out.push_str(&format!(
                    "\n== {} ==\n{}",
                    func.name,
                    func.chunk.disassemble()
                ));
            }
        }
        out
    }
}
