use crate::lexer::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Less => "<",
            BinOp::LessEq => "<=",
            BinOp::Greater => ">",
            BinOp::GreaterEq => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Str(String),
    Bool(bool),
    Nil,
    // Names and operators keep their position: they are what fails at run time.
    Name(String, Span),
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    Unary(UnOp, Box<Expr>, Span),
    Binary(BinOp, Box<Expr>, Box<Expr>, Span),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        at: Span,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
        at: Span,
    },
    Field {
        target: Box<Expr>,
        name: String,
        at: Span,
    },
    Func {
        name: Option<String>,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        at: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        value: Expr,
        at: Span,
    },
    Expr(Expr),
    Return {
        value: Option<Expr>,
        at: Span,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        otherwise: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    For {
        name: String,
        iter: Expr,
        body: Vec<Stmt>,
        at: Span,
    },
    Block(Vec<Stmt>),
}

// Printed back as source, which is how the parser's output is checked.
pub fn print_stmts(stmts: &[Stmt], indent: usize) -> String {
    stmts
        .iter()
        .map(|s| print_stmt(s, indent))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pad(indent: usize) -> String {
    "  ".repeat(indent)
}

fn print_block(body: &[Stmt], indent: usize) -> String {
    if body.is_empty() {
        return "{}".to_string();
    }
    format!("{{\n{}\n{}}}", print_stmts(body, indent + 1), pad(indent))
}

pub fn print_stmt(stmt: &Stmt, indent: usize) -> String {
    let line = match stmt {
        Stmt::Let { name, value, .. } => format!("let {name} = {};", print_expr_at(value, indent)),
        Stmt::Expr(e) => format!("{};", print_expr_at(e, indent)),
        Stmt::Return { value: Some(e), .. } => format!("return {};", print_expr_at(e, indent)),
        Stmt::Return { value: None, .. } => "return;".to_string(),
        Stmt::If {
            cond,
            then,
            otherwise,
        } => {
            let head = format!(
                "if {} {}",
                print_expr_at(cond, indent),
                print_block(then, indent)
            );
            if otherwise.is_empty() {
                head
            } else {
                format!("{head} else {}", print_block(otherwise, indent))
            }
        }
        Stmt::While { cond, body } => {
            format!(
                "while {} {}",
                print_expr_at(cond, indent),
                print_block(body, indent)
            )
        }
        Stmt::For {
            name, iter, body, ..
        } => {
            format!(
                "for {name} in {} {}",
                print_expr_at(iter, indent),
                print_block(body, indent)
            )
        }
        Stmt::Block(body) => print_block(body, indent),
    };
    format!("{}{line}", pad(indent))
}

pub fn print_expr(expr: &Expr) -> String {
    print_expr_at(expr, 0)
}

pub fn print_expr_at(expr: &Expr, indent: usize) -> String {
    match expr {
        Expr::Number(n) => format!("{n}"),
        Expr::Str(s) => format!("{s:?}"),
        Expr::Bool(b) => format!("{b}"),
        Expr::Nil => "nil".to_string(),
        Expr::Name(name, _) => name.clone(),
        Expr::List(items) => {
            format!(
                "[{}]",
                items.iter().map(print_expr).collect::<Vec<_>>().join(", ")
            )
        }
        Expr::Map(pairs) => {
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{}: {}", print_expr_at(k, indent), print_expr_at(v, indent)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
        Expr::Unary(UnOp::Neg, e, _) => format!("(-{})", print_expr_at(e, indent)),
        Expr::Unary(UnOp::Not, e, _) => format!("(not {})", print_expr_at(e, indent)),
        // Parentheses on every binary node, so the printed form shows how it grouped.
        Expr::Binary(op, l, r, _) => {
            format!(
                "({} {} {})",
                print_expr_at(l, indent),
                op.symbol(),
                print_expr_at(r, indent)
            )
        }
        Expr::Call { callee, args, .. } => {
            format!(
                "{}({})",
                print_expr_at(callee, indent),
                args.iter().map(print_expr).collect::<Vec<_>>().join(", ")
            )
        }
        Expr::Index { target, index, .. } => {
            format!(
                "{}[{}]",
                print_expr_at(target, indent),
                print_expr_at(index, indent)
            )
        }
        Expr::Field { target, name, .. } => format!("{}.{name}", print_expr_at(target, indent)),
        Expr::Func { name, params, body } => {
            let named = name.clone().unwrap_or_default();
            format!(
                "fn {named}({}) {}",
                params.join(", "),
                print_block(body, indent)
            )
        }
        Expr::Assign { target, value, .. } => {
            format!(
                "({} = {})",
                print_expr_at(target, indent),
                print_expr_at(value, indent)
            )
        }
    }
}
