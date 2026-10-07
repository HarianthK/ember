use crate::ast::{BinOp, Expr, Stmt, UnOp};
use crate::chunk::{Chunk, Function, Op, UpvalueRef, Value};
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
    // A captured local cannot just be popped when its block ends; it has to be closed.
    captured: bool,
}

// One per function being compiled. They form a stack, innermost last, so resolving a
// name can look outwards through every function the code is nested in.
// A loop being compiled, for break and continue to find their way out of.
struct Loop {
    // Locals deeper than this belong to the loop's body and end when break or continue leaves it.
    depth: usize,
    // A while loop continues at its condition, known at once; a for loop continues at the
    // step that advances its counter, which comes after the body, so those jumps are patched.
    continue_to: Option<u16>,
    continues: Vec<usize>,
    breaks: Vec<usize>,
}

struct FnState {
    chunk: Chunk,
    // Innermost last. Each function has its own, so break cannot leave a function.
    loops: Vec<Loop>,
    // Locals in declaration order; a local's index here is its stack slot at run time.
    locals: Vec<Local>,
    depth: usize,
    is_script: bool,
    upvalues: Vec<UpvalueRef>,
}

impl FnState {
    fn new(own_name: &str, is_script: bool) -> Self {
        FnState {
            chunk: Chunk::new(),
            loops: Vec::new(),
            // Slot 0 holds the function being run. Naming it after the function is what
            // lets a function call itself by name without capturing anything.
            locals: vec![Local {
                name: own_name.to_string(),
                depth: 0,
                captured: false,
            }],
            depth: 0,
            is_script,
            upvalues: Vec::new(),
        }
    }

    // Searched from the end, so an inner declaration shadows an outer one of the same name.
    fn resolve_local(&self, name: &str) -> Option<u16> {
        self.locals
            .iter()
            .rposition(|l| l.name == name)
            .map(|i| i as u16)
    }
}

pub struct Compiler {
    states: Vec<FnState>,
    // The span of the statement being compiled, for expressions that carry none of their own.
    at: Span,
    source: &'static str,
}

impl Compiler {
    fn st(&mut self) -> &mut FnState {
        self.states.last_mut().expect("a function being compiled")
    }

    fn st_ref(&self) -> &FnState {
        self.states.last().expect("a function being compiled")
    }

    fn emit(&mut self, op: Op) {
        let at = self.at;
        self.st().chunk.push(op, at);
    }

    fn name_constant(&mut self, name: &str) -> u16 {
        self.st().chunk.constant(Value::Str(name.to_string()))
    }

    // A name that is not a local of the function at `level` may be a local of a function
    // around it. Each function in between gets an upvalue, so the capture is passed inwards
    // one level at a time, and every closure only ever looks one level out.
    fn resolve_upvalue(&mut self, level: usize, name: &str) -> Option<u16> {
        if level == 0 {
            return None;
        }
        let outer = level - 1;
        if let Some(slot) = self.states[outer].resolve_local(name) {
            self.states[outer].locals[slot as usize].captured = true;
            return Some(self.add_upvalue(level, true, slot));
        }
        let index = self.resolve_upvalue(outer, name)?;
        Some(self.add_upvalue(level, false, index))
    }

    fn add_upvalue(&mut self, level: usize, is_local: bool, index: u16) -> u16 {
        let wanted = UpvalueRef { is_local, index };
        let upvalues = &mut self.states[level].upvalues;
        // Using a captured name twice must not capture it twice.
        if let Some(i) = upvalues.iter().position(|u| *u == wanted) {
            return i as u16;
        }
        upvalues.push(wanted);
        (upvalues.len() - 1) as u16
    }

    fn block(&mut self, body: &[Stmt]) -> Result<(), CompileError> {
        self.st().depth += 1;
        for stmt in body {
            self.stmt(stmt)?;
        }
        self.end_scope();
        Ok(())
    }

    fn declare(&mut self, name: &str) -> u16 {
        let depth = self.st_ref().depth;
        self.st().locals.push(Local {
            name: name.to_string(),
            depth,
            captured: false,
        });
        (self.st_ref().locals.len() - 1) as u16
    }

    fn end_scope(&mut self) {
        self.st().depth -= 1;
        // The scope's locals are still on the stack; take them off as it ends.
        loop {
            let depth = self.st_ref().depth;
            let Some(local) = self.st().locals.pop_if(|l| l.depth > depth) else {
                break;
            };
            self.emit(if local.captured {
                Op::CloseUpvalue
            } else {
                Op::Pop
            });
        }
    }

    // Ends every local the innermost loop's body has declared so far, without forgetting them:
    // the code after a break in the same block still compiles against them. Each is closed,
    // not just popped, so a closure that captured it keeps its value.
    fn leave_loop_body(&mut self) {
        let depth = self.st_ref().loops.last().expect("inside a loop").depth;
        let inside = self
            .st_ref()
            .locals
            .iter()
            .rev()
            .take_while(|l| l.depth > depth)
            .count();
        for _ in 0..inside {
            self.emit(Op::CloseUpvalue);
        }
    }

    fn begin_loop(&mut self, continue_to: Option<u16>) {
        let depth = self.st_ref().depth;
        self.st().loops.push(Loop {
            depth,
            continue_to,
            continues: Vec::new(),
            breaks: Vec::new(),
        });
    }

    // Call where the loop's continue lands, for a loop that did not know it at the start.
    fn patch_continues(&mut self) -> Result<(), CompileError> {
        let continues =
            std::mem::take(&mut self.st().loops.last_mut().expect("inside a loop").continues);
        for at in continues {
            self.patch(at)?;
        }
        Ok(())
    }

    // Call once the loop's own cleanup is emitted: a break lands just after it.
    fn end_loop(&mut self) -> Result<(), CompileError> {
        let finished = self.st().loops.pop().expect("inside a loop");
        for at in finished.breaks {
            self.patch(at)?;
        }
        Ok(())
    }

    fn constant(&mut self, value: Value) {
        let k = self.st().chunk.constant(value);
        self.emit(Op::Constant(k));
    }

    // `for x in seq` counts through seq with two hidden locals. Their names start with a
    // space, which no name in a program can, so the loop cannot be interfered with.
    fn for_loop(
        &mut self,
        name: &str,
        iter: &Expr,
        body: &[Stmt],
        at: Span,
    ) -> Result<(), CompileError> {
        self.at = at;
        self.st().depth += 1;
        self.expr(iter)?;
        self.at = at;
        self.emit(Op::Iterable);
        let seq = self.declare(" seq");
        self.constant(Value::Number(0.0));
        let i = self.declare(" i");
        let start = self.here()?;
        self.begin_loop(None);
        self.emit(Op::GetLocal(i));
        self.emit(Op::GetLocal(seq));
        self.emit(Op::Len);
        self.emit(Op::Less);
        let to_exit = self.jump(Op::JumpIfFalse);
        self.emit(Op::Pop);
        // A fresh scope each pass, so a closure made in the body captures that pass's item.
        self.st().depth += 1;
        self.emit(Op::GetLocal(seq));
        self.emit(Op::GetLocal(i));
        self.emit(Op::GetIndex);
        self.declare(name);
        for stmt in body {
            self.stmt(stmt)?;
        }
        self.at = at;
        self.end_scope();
        self.patch_continues()?;
        self.emit(Op::GetLocal(i));
        self.constant(Value::Number(1.0));
        self.emit(Op::Add);
        self.emit(Op::SetLocal(i));
        self.emit(Op::Pop);
        self.emit(Op::Jump(start));
        self.patch(to_exit)?;
        self.emit(Op::Pop);
        self.end_loop()?;
        self.end_scope();
        Ok(())
    }

    // Emits a jump whose target is not known yet; patch() fills it in once it is.
    fn jump(&mut self, make: fn(u16) -> Op) -> usize {
        self.emit(make(u16::MAX));
        self.st_ref().chunk.code.len() - 1
    }

    fn here(&self) -> Result<u16, CompileError> {
        u16::try_from(self.st_ref().chunk.code.len()).map_err(|_| CompileError {
            message: "this program is too long to jump across; the limit is 65535 instructions"
                .into(),
            at: self.at,
        })
    }

    fn patch(&mut self, at: usize) -> Result<(), CompileError> {
        let to = self.here()?;
        let code = &mut self.st().chunk.code;
        code[at] = match code[at] {
            Op::Jump(_) => Op::Jump(to),
            Op::JumpIfFalse(_) => Op::JumpIfFalse(to),
            other => unreachable!("patched a {other:?}, which is not a jump"),
        };
        Ok(())
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        match stmt {
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
                let depth = self.st_ref().depth;
                if depth == 0 {
                    let k = self.name_constant(name);
                    self.emit(Op::DefineGlobal(k));
                } else {
                    if self
                        .st_ref()
                        .locals
                        .iter()
                        .any(|l| l.depth == depth && &l.name == name)
                    {
                        return Err(CompileError {
                            message: format!("{name} is already declared in this block"),
                            at: *at,
                        });
                    }
                    // No instruction: the value just computed is already sitting in the local's slot.
                    self.st().locals.push(Local {
                        name: name.clone(),
                        depth,
                        captured: false,
                    });
                }
            }
            Stmt::Return { value, at } => {
                self.at = *at;
                if self.st_ref().is_script {
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
            Stmt::For {
                name,
                iter,
                body,
                at,
            } => self.for_loop(name, iter, body, *at)?,
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
                self.begin_loop(Some(start));
                self.expr(cond)?;
                let to_exit = self.jump(Op::JumpIfFalse);
                self.emit(Op::Pop);
                self.block(body)?;
                self.emit(Op::Jump(start));
                self.patch(to_exit)?;
                self.emit(Op::Pop);
                self.end_loop()?;
            }
            Stmt::Block(body) => self.block(body)?,
            Stmt::Break { at } | Stmt::Continue { at } => {
                self.at = *at;
                let is_break = matches!(stmt, Stmt::Break { .. });
                if self.st_ref().loops.is_empty() {
                    return Err(CompileError {
                        message: format!(
                            "{} is only allowed inside a loop",
                            if is_break { "break" } else { "continue" }
                        ),
                        at: *at,
                    });
                }
                self.leave_loop_body();
                let continue_to = self
                    .st_ref()
                    .loops
                    .last()
                    .expect("checked above")
                    .continue_to;
                match (is_break, continue_to) {
                    (false, Some(start)) => self.emit(Op::Jump(start)),
                    (false, None) => {
                        let jump = self.jump(Op::Jump);
                        self.st()
                            .loops
                            .last_mut()
                            .expect("checked above")
                            .continues
                            .push(jump);
                    }
                    (true, _) => {
                        let jump = self.jump(Op::Jump);
                        self.st()
                            .loops
                            .last_mut()
                            .expect("checked above")
                            .breaks
                            .push(jump);
                    }
                }
            }
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
        let mut state = FnState::new(&own_name, false);
        state.chunk.source = self.source;
        // Parameters are the first locals, in the slots the caller's arguments already occupy.
        state.depth = 1;
        for param in params {
            if state.locals.iter().skip(1).any(|l| &l.name == param) {
                return Err(CompileError {
                    message: format!("the parameter {param} is repeated"),
                    at,
                });
            }
            state.locals.push(Local {
                name: param.clone(),
                depth: 1,
                captured: false,
            });
        }
        self.states.push(state);
        // The body runs at the parameters' depth, so `let a` in it cannot silently hide parameter a.
        for stmt in body {
            self.stmt(stmt)?;
        }
        // Falling off the end returns nil. Return closes whatever the frame's locals had
        // captured, so nothing is popped or closed here.
        self.emit(Op::Nil);
        self.emit(Op::Return);
        let state = self.states.pop().expect("the state pushed above");
        let func = Function {
            name: if own_name.is_empty() {
                "anonymous".into()
            } else {
                own_name
            },
            arity,
            chunk: state.chunk,
            upvalues: state.upvalues,
        };
        let k = self.st().chunk.constant(Value::Function(Rc::new(func)));
        self.at = at;
        self.emit(Op::Closure(k));
        Ok(())
    }

    // Where a name lives, from nearest to furthest: this function, an enclosing one, global.
    fn variable(&mut self, name: &str, set: bool) {
        let level = self.states.len() - 1;
        if let Some(slot) = self.st_ref().resolve_local(name) {
            self.emit(if set {
                Op::SetLocal(slot)
            } else {
                Op::GetLocal(slot)
            });
        } else if let Some(i) = self.resolve_upvalue(level, name) {
            self.emit(if set {
                Op::SetUpvalue(i)
            } else {
                Op::GetUpvalue(i)
            });
        } else {
            // Every local this code could have meant, in this function or around it; hidden
            // ones start with a space and the script's own slot has no name, so both are left out.
            let mut names: Vec<String> = self
                .states
                .iter()
                .flat_map(|s| &s.locals)
                .map(|l| l.name.clone())
                .filter(|n| n.starts_with(|c: char| c.is_alphabetic() || c == '_'))
                .collect();
            names.sort();
            names.dedup();
            if !names.is_empty() {
                let at = self.st_ref().chunk.code.len();
                self.st().chunk.nearby.push((at, names));
            }
            let k = self.name_constant(name);
            self.emit(if set {
                Op::SetGlobal(k)
            } else {
                Op::GetGlobal(k)
            });
        }
    }

    fn expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::Number(n) => {
                let k = self.st().chunk.constant(Value::Number(*n));
                self.emit(Op::Constant(k));
            }
            Expr::Str(s) => {
                let k = self.st().chunk.constant(Value::Str(s.clone()));
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
                    BinOp::In => Op::In,
                    BinOp::And | BinOp::Or => unreachable!("handled above"),
                });
            }
            Expr::Name(name, at) => {
                self.at = *at;
                self.variable(name, false);
            }
            Expr::Assign { target, value, at } if matches!(target.as_ref(), Expr::Name(..)) => {
                let Expr::Name(name, _) = target.as_ref() else {
                    unreachable!()
                };
                self.expr(value)?;
                self.at = *at;
                // Assignment is an expression, so the value stays on the stack after it is stored.
                self.variable(name, true);
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
            Expr::List(items) => {
                for item in items {
                    self.expr(item)?;
                }
                let n = u16::try_from(items.len()).map_err(|_| CompileError {
                    message: "a list literal can hold at most 65535 items".into(),
                    at: self.at,
                })?;
                self.emit(Op::BuildList(n));
            }
            Expr::Index { target, index, at } => {
                self.expr(target)?;
                self.expr(index)?;
                self.at = *at;
                self.emit(Op::GetIndex);
            }
            Expr::Assign { target, value, at } if matches!(target.as_ref(), Expr::Index { .. }) => {
                let Expr::Index {
                    target: list,
                    index,
                    ..
                } = target.as_ref()
                else {
                    unreachable!()
                };
                self.expr(list)?;
                self.expr(index)?;
                self.expr(value)?;
                self.at = *at;
                self.emit(Op::SetIndex);
            }
            Expr::Map(pairs) => {
                for (key, value) in pairs {
                    self.expr(key)?;
                    self.expr(value)?;
                }
                let n = u16::try_from(pairs.len()).map_err(|_| CompileError {
                    message: "a map literal can hold at most 65535 entries".into(),
                    at: self.at,
                })?;
                self.emit(Op::BuildMap(n));
            }
            // m.name is m["name"]: the same instructions, with the name as a constant key.
            Expr::Field { target, name, at } => {
                self.expr(target)?;
                self.at = *at;
                self.constant(Value::Str(name.clone()));
                self.emit(Op::GetIndex);
            }
            Expr::Assign { target, value, at } => {
                let Expr::Field {
                    target: map, name, ..
                } = target.as_ref()
                else {
                    unreachable!("the parser only allows a name, an index or a field here");
                };
                self.expr(map)?;
                self.at = *at;
                self.constant(Value::Str(name.clone()));
                self.expr(value)?;
                self.at = *at;
                self.emit(Op::SetIndex);
            }
        }
        Ok(())
    }
}

// The whole program compiles to a function of no arguments, which the VM calls to start.
pub fn compile(program: &[Stmt]) -> Result<Rc<Function>, CompileError> {
    compile_script(program, false, "")
}

// The same for code from somewhere other than the program, whose errors should say so.
pub fn compile_from(program: &[Stmt], source: &'static str) -> Result<Rc<Function>, CompileError> {
    compile_script(program, false, source)
}

// For the REPL: a line ending in a bare expression returns its value instead of dropping it.
pub fn compile_repl(program: &[Stmt]) -> Result<Rc<Function>, CompileError> {
    compile_script(program, true, "")
}

fn compile_script(
    program: &[Stmt],
    keep_last: bool,
    source: &'static str,
) -> Result<Rc<Function>, CompileError> {
    let mut script = FnState::new("", true);
    script.chunk.source = source;
    let mut c = Compiler {
        states: vec![script],
        at: Span { line: 1, col: 1 },
        source,
    };
    let (last, rest) = match program.split_last() {
        Some((Stmt::Expr(e), rest)) if keep_last => (Some(e), rest),
        _ => (None, program),
    };
    for stmt in rest {
        c.stmt(stmt)?;
    }
    match last {
        Some(e) => c.expr(e)?,
        None => c.emit(Op::Nil),
    }
    c.emit(Op::Return);
    let state = c.states.pop().expect("the script's state");
    Ok(Rc::new(Function {
        name: "script".into(),
        arity: 0,
        chunk: state.chunk,
        upvalues: Vec::new(),
    }))
}
