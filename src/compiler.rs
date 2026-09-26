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

struct Local {
    name: String,
    depth: usize,
}

pub struct Compiler {
    chunk: Chunk,
    // The span of the statement being compiled, for expressions that carry none of their own.
    at: Span,
    // Locals in declaration order; a local's index here is its stack slot at run time.
    locals: Vec<Local>,
    depth: usize,
}

impl Compiler {
    fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            at: Span { line: 1, col: 1 },
            locals: Vec::new(),
            depth: 0,
        }
    }

    fn emit(&mut self, op: Op) {
        let at = self.at;
        self.chunk.push(op, at);
    }

    fn name_constant(&mut self, name: &str) -> u16 {
        self.chunk.constant(Value::Str(name.to_string()))
    }

    // Searched from the end, so an inner declaration shadows an outer one of the same name.
    fn resolve(&self, name: &str) -> Option<u16> {
        self.locals
            .iter()
            .rposition(|l| l.name == name)
            .map(|i| i as u16)
    }

    fn block(&mut self, body: &[Stmt]) -> Result<(), CompileError> {
        self.depth += 1;
        for stmt in body {
            self.stmt(stmt)?;
        }
        self.depth -= 1;
        // The block's locals are still on the stack; take them off as the scope ends.
        while self.locals.last().is_some_and(|l| l.depth > self.depth) {
            self.locals.pop();
            self.emit(Op::Pop);
        }
        Ok(())
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
            Stmt::Let { name, value, at } => {
                self.at = *at;
                // The value is compiled before the name exists, so `let x = x` reads the outer x.
                self.expr(value)?;
                self.at = *at;
                if self.depth == 0 {
                    let k = self.name_constant(name);
                    self.emit(Op::DefineGlobal(k));
                } else {
                    if self
                        .locals
                        .iter()
                        .any(|l| l.depth == self.depth && &l.name == name)
                    {
                        return Err(CompileError {
                            message: format!("{name} is already declared in this block"),
                            at: *at,
                        });
                    }
                    // No instruction: the value just computed is already sitting in the local's slot.
                    self.locals.push(Local {
                        name: name.clone(),
                        depth: self.depth,
                    });
                }
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
            Stmt::Block(body) => self.block(body)?,
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
            Expr::Name(name) => match self.resolve(name) {
                Some(slot) => self.emit(Op::GetLocal(slot)),
                None => {
                    let k = self.name_constant(name);
                    self.emit(Op::GetGlobal(k));
                }
            },
            Expr::Assign { target, value, at } if matches!(target.as_ref(), Expr::Name(_)) => {
                let Expr::Name(name) = target.as_ref() else {
                    unreachable!()
                };
                self.expr(value)?;
                self.at = *at;
                // Assignment is an expression, so the value stays on the stack after it is stored.
                match self.resolve(name) {
                    Some(slot) => self.emit(Op::SetLocal(slot)),
                    None => {
                        let k = self.name_constant(name);
                        self.emit(Op::SetGlobal(k));
                    }
                }
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
