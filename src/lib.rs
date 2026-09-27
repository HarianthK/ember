pub mod ast;
pub mod chunk;
pub mod compiler;
pub mod heap;
pub mod lexer;
pub mod parser;
pub mod vm;
pub mod walk;

use std::fmt;

// Any of the three stages can fail; the message already says where.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Runs one REPL entry on a VM that keeps its globals between entries. Returns the
/// value of a closing bare expression, as the REPL shows it, unless that value is nil.
pub fn repl_line(vm: &mut vm::Vm, src: &str) -> Result<Option<String>, Error> {
    let program = parser::parse(src).map_err(|e| Error(e.to_string()))?;
    let script = compiler::compile_repl(&program).map_err(|e| Error(e.to_string()))?;
    let value = vm.run(script).map_err(|e| Error(e.to_string()))?;
    Ok(match value {
        chunk::Value::Nil => None,
        v => Some(vm.heap.repr(&v)),
    })
}

/// Whether a REPL entry stopped short, like an open brace, and should get another line
/// rather than an error. A real mistake, like `1 +* 2`, is reported straight away.
pub fn needs_more(src: &str) -> bool {
    match parser::parse(src) {
        Ok(_) => false,
        Err(e) => e.message.contains("never closed") || e.message.contains("end of input"),
    }
}

/// Parses, compiles and runs a program, returning what it printed.
pub fn run(src: &str) -> Result<Vec<String>, Error> {
    let program = parser::parse(src).map_err(|e| Error(e.to_string()))?;
    let script = compiler::compile(&program).map_err(|e| Error(e.to_string()))?;
    let mut vm = vm::Vm::new();
    vm.run(script).map_err(|e| Error(e.to_string()))?;
    Ok(vm.output)
}
