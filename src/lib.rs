pub mod ast;
pub mod chunk;
pub mod compiler;
pub mod heap;
pub mod lexer;
pub mod parser;
pub mod vm;

use std::fmt;

// Any of the three stages can fail; the message already says where.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
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
