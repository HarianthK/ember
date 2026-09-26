use crate::lexer::Span;
use crate::vm::Vm;
use std::fmt;
use std::rc::Rc;

// Rc for now; phase 4 replaces it with handles into a heap the collector owns.
#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    Nil,
    Function(Rc<Function>),
    Native(Rc<Native>),
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
}

// Two functions are equal only if they are the same function, never by comparing code.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Nil, Value::Nil) => true,
            (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
            (Value::Native(a), Value::Native(b)) => Rc::ptr_eq(a, b),
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
            Value::Function(_) | Value::Native(_) => "function",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Nil => write!(f, "nil"),
            Value::Function(func) => write!(f, "<fn {}>", func.name),
            Value::Native(native) => write!(f, "<native {}>", native.name),
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
    Pop,
    Return,
    // Globals are looked up by name, the operand being the name's constant.
    DefineGlobal(u16),
    GetGlobal(u16),
    SetGlobal(u16),
    // Locals live on the stack; the operand is the slot, so no name is looked up at run time.
    GetLocal(u16),
    SetLocal(u16),
    // The operand is the argument count; the function sits on the stack just below the arguments.
    Call(u8),
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
                Op::Call(argc) => format!("{:<12} {argc:>4}", "CALL"),
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
