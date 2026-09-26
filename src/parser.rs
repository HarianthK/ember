use crate::ast::{BinOp, Expr, Stmt, UnOp};
use crate::lexer::{LexError, Span, Tok, Token, tokenize};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub at: Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.message, self.at)
    }
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError {
            message: e.message,
            at: e.at,
        }
    }
}

// Binding powers. Higher binds tighter; call and index are handled separately.
fn infix_power(tok: &Tok) -> Option<(BinOp, u8)> {
    let pair = match tok {
        Tok::Or => (BinOp::Or, 1),
        Tok::And => (BinOp::And, 2),
        Tok::Eq => (BinOp::Eq, 3),
        Tok::NotEq => (BinOp::NotEq, 3),
        Tok::Less => (BinOp::Less, 4),
        Tok::LessEq => (BinOp::LessEq, 4),
        Tok::Greater => (BinOp::Greater, 4),
        Tok::GreaterEq => (BinOp::GreaterEq, 4),
        Tok::Plus => (BinOp::Add, 5),
        Tok::Minus => (BinOp::Sub, 5),
        Tok::Star => (BinOp::Mul, 6),
        Tok::Slash => (BinOp::Div, 6),
        Tok::Percent => (BinOp::Rem, 6),
        _ => return None,
    };
    Some(pair)
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(src: &str) -> Result<Self, ParseError> {
        Ok(Parser {
            tokens: tokenize(src)?,
            pos: 0,
        })
    }

    fn peek(&self) -> &Tok {
        &self.tokens[self.pos].tok
    }

    fn at(&self) -> Span {
        self.tokens[self.pos].at
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, want: &Tok) -> bool {
        if self.peek() == want {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, want: Tok) -> Result<Token, ParseError> {
        if self.peek() == &want {
            Ok(self.advance())
        } else {
            Err(ParseError {
                message: format!("I expected {want} here, but found {}", self.peek()),
                at: self.at(),
            })
        }
    }

    fn name(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Tok::Name(name) => {
                self.advance();
                Ok(name)
            }
            other => Err(ParseError {
                message: format!("I expected a name here, but found {other}"),
                at: self.at(),
            }),
        }
    }

    pub fn program(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut out = Vec::new();
        while self.peek() != &Tok::Eof {
            out.push(self.statement()?);
        }
        Ok(out)
    }

    fn block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect(Tok::LBrace)?;
        let mut out = Vec::new();
        while self.peek() != &Tok::RBrace {
            if self.peek() == &Tok::Eof {
                return Err(ParseError {
                    message: "this block is never closed".into(),
                    at: self.at(),
                });
            }
            out.push(self.statement()?);
        }
        self.expect(Tok::RBrace)?;
        Ok(out)
    }

    fn statement(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().clone() {
            Tok::Let => {
                let at = self.at();
                self.advance();
                let name = self.name()?;
                self.expect(Tok::Assign)?;
                let value = self.expression()?;
                self.eat(&Tok::Semicolon);
                Ok(Stmt::Let { name, value, at })
            }
            Tok::Return => {
                let at = self.at();
                self.advance();
                let value = if matches!(self.peek(), Tok::Semicolon | Tok::RBrace | Tok::Eof) {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.eat(&Tok::Semicolon);
                Ok(Stmt::Return { value, at })
            }
            Tok::If => {
                self.advance();
                let cond = self.expression()?;
                let then = self.block()?;
                let otherwise = if self.eat(&Tok::Else) {
                    // "else if" chains without needing braces around the inner if.
                    if self.peek() == &Tok::If {
                        vec![self.statement()?]
                    } else {
                        self.block()?
                    }
                } else {
                    Vec::new()
                };
                Ok(Stmt::If {
                    cond,
                    then,
                    otherwise,
                })
            }
            Tok::While => {
                self.advance();
                let cond = self.expression()?;
                let body = self.block()?;
                Ok(Stmt::While { cond, body })
            }
            Tok::For => {
                let at = self.at();
                self.advance();
                let name = self.name()?;
                self.expect(Tok::In)?;
                let iter = self.expression()?;
                let body = self.block()?;
                Ok(Stmt::For {
                    name,
                    iter,
                    body,
                    at,
                })
            }
            // A named function is a statement; an anonymous one is an expression.
            Tok::Fn if matches!(self.tokens[self.pos + 1].tok, Tok::Name(_)) => {
                let at = self.at();
                let func = self.function()?;
                let name = match &func {
                    Expr::Func {
                        name: Some(name), ..
                    } => name.clone(),
                    _ => unreachable!("function() just parsed a named function"),
                };
                Ok(Stmt::Let {
                    name,
                    value: func,
                    at,
                })
            }
            Tok::LBrace => Ok(Stmt::Block(self.block()?)),
            _ => {
                let expr = self.expression()?;
                self.eat(&Tok::Semicolon);
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn function(&mut self) -> Result<Expr, ParseError> {
        self.expect(Tok::Fn)?;
        let name = match self.peek().clone() {
            Tok::Name(name) => {
                self.advance();
                Some(name)
            }
            _ => None,
        };
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        while self.peek() != &Tok::RParen {
            params.push(self.name()?);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        let body = self.block()?;
        Ok(Expr::Func { name, params, body })
    }

    pub fn expression(&mut self) -> Result<Expr, ParseError> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, ParseError> {
        let left = self.binary(0)?;
        if self.peek() == &Tok::Assign {
            let at = self.at();
            self.advance();
            // Right associative, so a = b = c parses as a = (b = c).
            let value = self.assignment()?;
            return match left {
                Expr::Name(..) | Expr::Index { .. } | Expr::Field { .. } => Ok(Expr::Assign {
                    target: Box::new(left),
                    value: Box::new(value),
                    at,
                }),
                _ => Err(ParseError {
                    message:
                        "I cannot assign to this; the left side must be a name, an index or a field"
                            .into(),
                    at,
                }),
            };
        }
        Ok(left)
    }

    fn binary(&mut self, min_power: u8) -> Result<Expr, ParseError> {
        let mut left = self.unary()?;
        while let Some((op, power)) = infix_power(self.peek()) {
            if power < min_power {
                break;
            }
            let at = self.at();
            self.advance();
            // Left associative: the right side stops at anything binding this loosely.
            let right = self.binary(power + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right), at);
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Tok::Minus => {
                let at = self.at();
                self.advance();
                Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?), at))
            }
            Tok::Not => {
                let at = self.at();
                self.advance();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?), at))
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.primary()?;
        loop {
            let at = self.at();
            match self.peek() {
                Tok::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    while self.peek() != &Tok::RParen {
                        args.push(self.expression()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RParen)?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                        at,
                    };
                }
                Tok::LBracket => {
                    self.advance();
                    let index = self.expression()?;
                    self.expect(Tok::RBracket)?;
                    expr = Expr::Index {
                        target: Box::new(expr),
                        index: Box::new(index),
                        at,
                    };
                }
                Tok::Dot => {
                    self.advance();
                    let name = self.name()?;
                    expr = Expr::Field {
                        target: Box::new(expr),
                        name,
                        at,
                    };
                }
                _ => return Ok(expr),
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let at = self.at();
        match self.peek().clone() {
            Tok::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Tok::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Tok::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Tok::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Tok::Nil => {
                self.advance();
                Ok(Expr::Nil)
            }
            Tok::Name(name) => {
                self.advance();
                Ok(Expr::Name(name, at))
            }
            Tok::Fn => self.function(),
            Tok::LParen => {
                self.advance();
                let inner = self.expression()?;
                self.expect(Tok::RParen)?;
                Ok(inner)
            }
            Tok::LBracket => {
                self.advance();
                let mut items = Vec::new();
                while self.peek() != &Tok::RBracket {
                    items.push(self.expression()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RBracket)?;
                Ok(Expr::List(items))
            }
            Tok::LBrace => {
                self.advance();
                let mut pairs = Vec::new();
                while self.peek() != &Tok::RBrace {
                    let key = self.expression()?;
                    self.expect(Tok::Colon)?;
                    let value = self.expression()?;
                    pairs.push((key, value));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RBrace)?;
                Ok(Expr::Map(pairs))
            }
            other => Err(ParseError {
                message: format!("I cannot start an expression with {other}"),
                at,
            }),
        }
    }
}

pub fn parse(src: &str) -> Result<Vec<Stmt>, ParseError> {
    Parser::new(src)?.program()
}
