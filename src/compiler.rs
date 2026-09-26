use crate::ast::{BinOp, Expr, Stmt, UnOp};
use crate::chunk::{Chunk, Function, Op, Value};
use crate::lexer::Span;
use std::fmt;
use std::rc::Rc;

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

// One of these per function being compiled; a nested function gets a fresh one.
pub struct Compiler {
    chunk: Chunk,
    // The span of the statement being compiled, for expressions that carry none of their own.
    at: Span,
    // Locals in declaration order; a local's index here is its stack slot at run time.
    locals: Vec<Local>,
    depth: usize,
    is_script: bool,
    // Locals of the functions around this one, which it cannot reach until closures exist.
    enclosing: Vec<String>,
}

impl Compiler {
    fn new(own_name: &str, is_script: bool, enclosing: Vec<String>, at: Span) -> Self {
        Compiler {
            chunk: Chunk::new(),
            at,
            // Slot 0 holds the function being run. Naming it after the function is what
            // lets a function call itself before closures can capture anything.
            locals: vec![Local {
                name: own_name.to_string(),
                depth: 0,
            }],
            depth: 0,
            is_script,
            enclosing,
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

    fn check_reachable(&self, name: &str) -> Result<(), CompileError> {
        // Without this, the name would quietly fall through to a global of the same name.
        if self.enclosing.iter().any(|n| n == name) {
            return Err(CompileError {
                message: format!(
                    "{name} belongs to an enclosing function; capturing it needs closures, which come in phase 3"
                ),
                at: self.at,
            });
        }
        Ok(())
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

    // Emits a jump whose target is not known yet; patch() fills it in once it is.
    fn jump(&mut self, make: fn(u16) -> Op) -> usize {
        self.emit(make(u16::MAX));
        self.chunk.code.len() - 1
    }

    fn here(&self) -> Result<u16, CompileError> {
        u16::try_from(self.chunk.code.len()).map_err(|_| CompileError {
            message: "this program is too long to jump across; the limit is 65535 instructions"
                .into(),
            at: self.at,
        })
    }

    fn patch(&mut self, at: usize) -> Result<(), CompileError> {
        let to = self.here()?;
        self.chunk.code[at] = match self.chunk.code[at] {
            Op::Jump(_) => Op::Jump(to),
            Op::JumpIfFalse(_) => Op::JumpIfFalse(to),
            other => unreachable!("patched a {other:?}, which is not a jump"),
        };
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
            Stmt::Expr(Expr::Call { callee, args, at }) if matches!(callee.as_ref(), Expr::Name(n, _) if n == "print") =>
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
            Stmt::Return { value, at } => {
                self.at = *at;
                if self.is_script {
                    return Err(CompileError {
                        message: "return is only allowed inside a function".into(),
                        at: *at,
                    });
                }
                match value {
                    Some(v) => self.expr(v)?,
                    None => self.emit(Op::Nil),
                }
                self.at = *at;
                self.emit(Op::Return);
            }
            Stmt::For { at, .. } => {
                self.at = *at;
                return Err(self.not_yet("for"));
            }
            Stmt::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond)?;
                let to_else = self.jump(Op::JumpIfFalse);
                self.emit(Op::Pop);
                self.block(then)?;
                let to_end = self.jump(Op::Jump);
                self.patch(to_else)?;
                self.emit(Op::Pop);
                self.block(otherwise)?;
                self.patch(to_end)?;
            }
            Stmt::While { cond, body } => {
                let start = self.here()?;
                self.expr(cond)?;
                let to_exit = self.jump(Op::JumpIfFalse);
                self.emit(Op::Pop);
                self.block(body)?;
                self.emit(Op::Jump(start));
                self.patch(to_exit)?;
                self.emit(Op::Pop);
            }
            Stmt::Block(body) => self.block(body)?,
        }
        Ok(())
    }

    fn function(
        &mut self,
        name: &Option<String>,
        params: &[String],
        body: &[Stmt],
    ) -> Result<(), CompileError> {
        let at = self.at;
        let own_name = name.clone().unwrap_or_default();
        let arity = u8::try_from(params.len()).map_err(|_| CompileError {
            message: format!(
                "a function can take at most 255 parameters, not {}",
                params.len()
            ),
            at,
        })?;
        // Every declared local here is out of reach for the function inside it. Slot 0 is left
        // out: a named function is usually a global too, and inner code may call it by that name.
        let mut enclosing = self.enclosing.clone();
        enclosing.extend(self.locals.iter().skip(1).map(|l| l.name.clone()));
        let mut inner = Compiler::new(&own_name, false, enclosing, at);
        // Parameters are the first locals, in the slots the caller's arguments already occupy.
        inner.depth = 1;
        for param in params {
            if inner.locals.iter().skip(1).any(|l| &l.name == param) {
                return Err(CompileError {
                    message: format!("the parameter {param} is repeated"),
                    at,
                });
            }
            inner.locals.push(Local {
                name: param.clone(),
                depth: 1,
            });
        }
        // The body runs at the parameters' depth, so `let a` in it cannot silently hide parameter a.
        for stmt in body {
            inner.stmt(stmt)?;
        }
        // Falling off the end returns nil. The frame's locals go with the frame, so nothing is popped.
        inner.emit(Op::Nil);
        inner.emit(Op::Return);
        let func = Function {
            name: if own_name.is_empty() {
                "anonymous".into()
            } else {
                own_name
            },
            arity,
            chunk: inner.chunk,
        };
        let k = self.chunk.constant(Value::Function(Rc::new(func)));
        self.at = at;
        self.emit(Op::Constant(k));
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
            Expr::Unary(op, operand, at) => {
                self.expr(operand)?;
                self.at = *at;
                self.emit(match op {
                    UnOp::Neg => Op::Neg,
                    UnOp::Not => Op::Not,
                });
            }
            // The right side only runs when the left does not decide it, and the result is
            // whichever operand decided, so `name or "default"` works as it does in Python.
            Expr::Binary(BinOp::And, left, right, at) => {
                self.expr(left)?;
                self.at = *at;
                let to_end = self.jump(Op::JumpIfFalse);
                self.emit(Op::Pop);
                self.expr(right)?;
                self.patch(to_end)?;
            }
            Expr::Binary(BinOp::Or, left, right, at) => {
                self.expr(left)?;
                self.at = *at;
                let to_right = self.jump(Op::JumpIfFalse);
                let to_end = self.jump(Op::Jump);
                self.patch(to_right)?;
                self.emit(Op::Pop);
                self.expr(right)?;
                self.patch(to_end)?;
            }
            Expr::Binary(op, left, right, at) => {
                // Operands in source order, so the stack holds left then right when the op runs.
                self.expr(left)?;
                self.expr(right)?;
                self.at = *at;
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
            Expr::Name(name, at) => {
                self.at = *at;
                match self.resolve(name) {
                    Some(slot) => self.emit(Op::GetLocal(slot)),
                    None => {
                        self.check_reachable(name)?;
                        let k = self.name_constant(name);
                        self.emit(Op::GetGlobal(k));
                    }
                }
            }
            Expr::Assign { target, value, at } if matches!(target.as_ref(), Expr::Name(..)) => {
                let Expr::Name(name, _) = target.as_ref() else {
                    unreachable!()
                };
                self.expr(value)?;
                self.at = *at;
                // Assignment is an expression, so the value stays on the stack after it is stored.
                match self.resolve(name) {
                    Some(slot) => self.emit(Op::SetLocal(slot)),
                    None => {
                        self.check_reachable(name)?;
                        let k = self.name_constant(name);
                        self.emit(Op::SetGlobal(k));
                    }
                }
            }
            Expr::Call { callee, args, at } => {
                // The function first, then its arguments above it, which become its first locals.
                self.expr(callee)?;
                for arg in args {
                    self.expr(arg)?;
                }
                self.at = *at;
                let argc = u8::try_from(args.len()).map_err(|_| CompileError {
                    message: format!("a call can pass at most 255 arguments, not {}", args.len()),
                    at: *at,
                })?;
                self.emit(Op::Call(argc));
            }
            Expr::Func { name, params, body } => self.function(name, params, body)?,
            Expr::Index { at, .. } | Expr::Field { at, .. } | Expr::Assign { at, .. } => {
                self.at = *at;
                let what = match expr {
                    Expr::Index { .. } => "indexing",
                    Expr::Field { .. } => "a field",
                    _ => "assignment",
                };
                return Err(self.not_yet(what));
            }
            Expr::List(_) => return Err(self.not_yet("a list")),
            Expr::Map(_) => return Err(self.not_yet("a map")),
        }
        Ok(())
    }
}

// The whole program compiles to a function of no arguments, which the VM calls to start.
pub fn compile(program: &[Stmt]) -> Result<Rc<Function>, CompileError> {
    let mut c = Compiler::new("", true, Vec::new(), Span { line: 1, col: 1 });
    for stmt in program {
        c.stmt(stmt)?;
    }
    c.emit(Op::Nil);
    c.emit(Op::Return);
    Ok(Rc::new(Function {
        name: "script".into(),
        arity: 0,
        chunk: c.chunk,
    }))
}
