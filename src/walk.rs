// A tree-walking interpreter over the same syntax tree, for measuring the VM against.
// It runs only what the benchmarks use: numbers, booleans, variables, functions, if, while.
use crate::ast::{BinOp, Expr, Stmt, UnOp};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
enum V {
    Num(f64),
    Bool(bool),
    Nil,
    Func(Rc<FuncDef>, Rc<Env>),
}

struct FuncDef {
    params: Vec<String>,
    body: Vec<Stmt>,
}

// The textbook design: every scope is a hash map with a pointer to the one around it, and
// every name is found by walking outwards. The VM exists to avoid exactly this.
struct Env {
    vars: RefCell<HashMap<String, V>>,
    parent: Option<Rc<Env>>,
}

impl Env {
    fn new(parent: Option<Rc<Env>>) -> Rc<Env> {
        Rc::new(Env {
            vars: RefCell::new(HashMap::new()),
            parent,
        })
    }

    fn get(&self, name: &str) -> Result<V, String> {
        if let Some(v) = self.vars.borrow().get(name) {
            return Ok(v.clone());
        }
        match &self.parent {
            Some(p) => p.get(name),
            None => Err(format!("{name} is not defined")),
        }
    }

    fn set(&self, name: &str, value: V) -> Result<(), String> {
        if let Some(slot) = self.vars.borrow_mut().get_mut(name) {
            *slot = value;
            return Ok(());
        }
        match &self.parent {
            Some(p) => p.set(name, value),
            None => Err(format!("{name} is not defined")),
        }
    }
}

enum Flow {
    Normal,
    Return(V),
}

pub struct Walker {
    pub output: Vec<String>,
}

fn show(v: &V) -> String {
    match v {
        V::Num(n) => format!("{n}"),
        V::Bool(b) => format!("{b}"),
        V::Nil => "nil".into(),
        V::Func(..) => "<fn>".into(),
    }
}

fn truthy(v: &V) -> bool {
    !matches!(v, V::Nil | V::Bool(false))
}

impl Walker {
    pub fn run(src: &str) -> Result<Vec<String>, String> {
        let program = crate::parser::parse(src).map_err(|e| e.to_string())?;
        let mut w = Walker { output: Vec::new() };
        w.block(&program, &Env::new(None))?;
        Ok(w.output)
    }

    fn block(&mut self, stmts: &[Stmt], env: &Rc<Env>) -> Result<Flow, String> {
        for stmt in stmts {
            if let Flow::Return(v) = self.stmt(stmt, env)? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Normal)
    }

    fn stmt(&mut self, stmt: &Stmt, env: &Rc<Env>) -> Result<Flow, String> {
        match stmt {
            Stmt::Let { name, value, .. } => {
                // Declared first, so a named function can find itself when it recurses.
                env.vars.borrow_mut().insert(name.clone(), V::Nil);
                let v = self.expr(value, env)?;
                env.vars.borrow_mut().insert(name.clone(), v);
            }
            Stmt::Expr(e) => {
                self.expr(e, env)?;
            }
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.expr(e, env)?,
                    None => V::Nil,
                };
                return Ok(Flow::Return(v));
            }
            Stmt::If {
                cond,
                then,
                otherwise,
            } => {
                let branch = if truthy(&self.expr(cond, env)?) {
                    then
                } else {
                    otherwise
                };
                return self.block(branch, &Env::new(Some(Rc::clone(env))));
            }
            Stmt::While { cond, body } => {
                while truthy(&self.expr(cond, env)?) {
                    if let Flow::Return(v) = self.block(body, &Env::new(Some(Rc::clone(env))))? {
                        return Ok(Flow::Return(v));
                    }
                }
            }
            Stmt::Block(body) => return self.block(body, &Env::new(Some(Rc::clone(env)))),
            Stmt::For { .. } => return Err("the tree-walker does not run for loops".into()),
            Stmt::Break { .. } | Stmt::Continue { .. } => {
                return Err("the tree-walker does not run break or continue".into());
            }
        }
        Ok(Flow::Normal)
    }

    fn expr(&mut self, expr: &Expr, env: &Rc<Env>) -> Result<V, String> {
        Ok(match expr {
            Expr::Number(n) => V::Num(*n),
            Expr::Bool(b) => V::Bool(*b),
            Expr::Nil => V::Nil,
            Expr::Name(name, _) => env.get(name)?,
            Expr::Unary(op, e, _) => match (op, self.expr(e, env)?) {
                (UnOp::Neg, V::Num(n)) => V::Num(-n),
                (UnOp::Not, v) => V::Bool(!truthy(&v)),
                _ => return Err("- needs a number".into()),
            },
            Expr::Binary(BinOp::And, l, r, _) => {
                let left = self.expr(l, env)?;
                if truthy(&left) {
                    self.expr(r, env)?
                } else {
                    left
                }
            }
            Expr::Binary(BinOp::Or, l, r, _) => {
                let left = self.expr(l, env)?;
                if truthy(&left) {
                    left
                } else {
                    self.expr(r, env)?
                }
            }
            Expr::Binary(op, l, r, _) => {
                let (V::Num(a), V::Num(b)) = (self.expr(l, env)?, self.expr(r, env)?) else {
                    return Err(format!("{} needs two numbers", op.symbol()));
                };
                match op {
                    BinOp::Add => V::Num(a + b),
                    BinOp::Sub => V::Num(a - b),
                    BinOp::Mul => V::Num(a * b),
                    BinOp::Div => V::Num(a / b),
                    BinOp::Rem => V::Num(a % b),
                    BinOp::Eq => V::Bool(a == b),
                    BinOp::NotEq => V::Bool(a != b),
                    BinOp::Less => V::Bool(a < b),
                    BinOp::LessEq => V::Bool(a <= b),
                    BinOp::Greater => V::Bool(a > b),
                    BinOp::GreaterEq => V::Bool(a >= b),
                    BinOp::In => return Err("the tree-walker does not run in".into()),
                    BinOp::And | BinOp::Or => unreachable!("handled above"),
                }
            }
            Expr::Assign { target, value, .. } => {
                let Expr::Name(name, _) = target.as_ref() else {
                    return Err("the tree-walker only assigns to names".into());
                };
                let v = self.expr(value, env)?;
                env.set(name, v.clone())?;
                v
            }
            Expr::Func { params, body, .. } => V::Func(
                Rc::new(FuncDef {
                    params: params.clone(),
                    body: body.clone(),
                }),
                Rc::clone(env),
            ),
            Expr::Call { callee, args, .. } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.expr(a, env)?);
                }
                if matches!(callee.as_ref(), Expr::Name(n, _) if n == "print") {
                    let line = values.iter().map(show).collect::<Vec<_>>().join(" ");
                    self.output.push(line);
                    return Ok(V::Nil);
                }
                let V::Func(def, captured) = self.expr(callee, env)? else {
                    return Err("only a function can be called".into());
                };
                let frame = Env::new(Some(captured));
                for (p, v) in def.params.iter().zip(values) {
                    frame.vars.borrow_mut().insert(p.clone(), v);
                }
                match self.block(&def.body, &frame)? {
                    Flow::Return(v) => v,
                    Flow::Normal => V::Nil,
                }
            }
            _ => return Err("the tree-walker does not run strings, lists or maps".into()),
        })
    }
}
