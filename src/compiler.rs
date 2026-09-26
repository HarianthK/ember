use crate::ast::{BinOp, Expr, Stmt, UnOp};
use crate::chunk::{Chunk, Op, Value};
use crate::lexer::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    pub message: String,
    pub at: Span,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.message, self.at)
    }
}

pub struct Compiler {
    chunk: Chunk,
    // The span of the statement being compiled, for expressions that carry none of their own.
    at: Span,
}

impl Compiler {
    fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            at: Span { line: 1, col: 1 },
        }
    }

    fn emit(&mut self, op: Op) {
        let at = self.at;
        self.chunk.push(op, at);
    }

    fn not_yet(&self, what: &str) -> CompileError {
        CompileError {
            message: format!("{what} is not compiled yet"),
            at: self.at,
        }
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        match stmt {
            // print is a call to a function that does not exist yet, so it is spelled as an instruction until then.
            Stmt::Expr(Expr::Call { callee, args, at }) if matches!(callee.as_ref(), Expr::Name(n) if n == "print") =>
            {
                self.at = *at;
                if args.len() != 1 {
                    return Err(CompileError {
                        message: format!("print takes one value, not {}", args.len()),
                        at: *at,
                    });
                }
                self.expr(&args[0])?;
                self.emit(Op::Print);
            }
            Stmt::Expr(e) => {
                self.expr(e)?;
                // An expression statement leaves its value on the stack; nothing wants it.
                self.emit(Op::Pop);
            }
            Stmt::Let { at, .. } => {
                self.at = *at;
                return Err(self.not_yet("let"));
            }
            Stmt::Return { at, .. } => {
                self.at = *at;
                return Err(self.not_yet("return"));
            }
            Stmt::For { at, .. } => {
                self.at = *at;
                return Err(self.not_yet("for"));
            }
            Stmt::If { .. } => return Err(self.not_yet("if")),
            Stmt::While { .. } => return Err(self.not_yet("while")),
            Stmt::Block(_) => return Err(self.not_yet("a block")),
        }
        Ok(())
    }

    fn expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::Number(n) => {
                let k = self.chunk.constant(Value::Number(*n));
                self.emit(Op::Constant(k));
            }
            Expr::Str(s) => {
                let k = self.chunk.constant(Value::Str(s.clone()));
                self.emit(Op::Constant(k));
            }
            Expr::Bool(true) => self.emit(Op::True),
            Expr::Bool(false) => self.emit(Op::False),
            Expr::Nil => self.emit(Op::Nil),
            Expr::Unary(op, operand) => {
                self.expr(operand)?;
                self.emit(match op {
                    UnOp::Neg => Op::Neg,
                    UnOp::Not => Op::Not,
                });
            }
            Expr::Binary(BinOp::And | BinOp::Or, ..) => return Err(self.not_yet("and/or")),
            Expr::Binary(op, left, right) => {
                // Operands in source order, so the stack holds left then right when the op runs.
                self.expr(left)?;
                self.expr(right)?;
                self.emit(match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Rem => Op::Rem,
                    BinOp::Eq => Op::Eq,
                    BinOp::NotEq => Op::NotEq,
                    BinOp::Less => Op::Less,
                    BinOp::LessEq => Op::LessEq,
                    BinOp::Greater => Op::Greater,
                    BinOp::GreaterEq => Op::GreaterEq,
                    BinOp::And | BinOp::Or => unreachable!("handled above"),
                });
            }
            Expr::Call { at, .. }
            | Expr::Index { at, .. }
            | Expr::Field { at, .. }
            | Expr::Assign { at, .. } => {
                self.at = *at;
                let what = match expr {
                    Expr::Call { .. } => "a call",
                    Expr::Index { .. } => "indexing",
                    Expr::Field { .. } => "a field",
                    _ => "assignment",
                };
                return Err(self.not_yet(what));
            }
            Expr::Name(_) => return Err(self.not_yet("a variable")),
            Expr::List(_) => return Err(self.not_yet("a list")),
            Expr::Map(_) => return Err(self.not_yet("a map")),
            Expr::Func { .. } => return Err(self.not_yet("a function")),
        }
        Ok(())
    }
}

pub fn compile(program: &[Stmt]) -> Result<Chunk, CompileError> {
    let mut c = Compiler::new();
    for stmt in program {
        c.stmt(stmt)?;
    }
    c.emit(Op::Return);
    Ok(c.chunk)
}
