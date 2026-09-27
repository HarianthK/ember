use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // literals and names
    Number(f64),
    Str(String),
    Name(String),
    // keywords
    Let,
    Fn,
    Return,
    If,
    Else,
    While,
    For,
    In,
    True,
    False,
    Nil,
    And,
    Or,
    Not,
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Dot,
    // operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Tok::Number(n) => return write!(f, "{n}"),
            Tok::Str(s) => return write!(f, "\"{s}\""),
            Tok::Name(s) => return write!(f, "{s}"),
            Tok::Let => "let",
            Tok::Fn => "fn",
            Tok::Return => "return",
            Tok::If => "if",
            Tok::Else => "else",
            Tok::While => "while",
            Tok::For => "for",
            Tok::In => "in",
            Tok::True => "true",
            Tok::False => "false",
            Tok::Nil => "nil",
            Tok::And => "and",
            Tok::Or => "or",
            Tok::Not => "not",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::Comma => ",",
            Tok::Semicolon => ";",
            Tok::Colon => ":",
            Tok::Dot => ".",
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::Assign => "=",
            Tok::Eq => "==",
            Tok::NotEq => "!=",
            Tok::Less => "<",
            Tok::LessEq => "<=",
            Tok::Greater => ">",
            Tok::GreaterEq => ">=",
            Tok::Eof => "end of input",
        };
        write!(f, "{text}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.col)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub at: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub at: Span,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.message, self.at)
    }
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> u8 {
        *self.src.get(self.pos).unwrap_or(&0)
    }

    fn peek_next(&self) -> u8 {
        *self.src.get(self.pos + 1).unwrap_or(&0)
    }

    fn here(&self) -> Span {
        Span {
            line: self.line,
            col: self.col,
        }
    }

    fn bump(&mut self) -> u8 {
        let c = self.peek();
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
            self.col = 1;
        } else if c & 0xC0 != 0x80 {
            // Continuation bytes of a multi-byte character do not start a new column.
            self.col += 1;
        }
        c
    }

    fn skip_blank(&mut self) {
        loop {
            match self.peek() {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.bump();
                }
                // Comments run to the end of the line; there is no block comment.
                b'/' if self.peek_next() == b'/' => {
                    while self.peek() != b'\n' && self.peek() != 0 {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    fn number(&mut self) -> Result<Token, LexError> {
        let at = self.here();
        let start = self.pos;
        while self.peek().is_ascii_digit() {
            self.bump();
        }
        if self.peek() == b'.' && self.peek_next().is_ascii_digit() {
            self.bump();
            while self.peek().is_ascii_digit() {
                self.bump();
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        match text.parse::<f64>() {
            Ok(n) => Ok(Token {
                tok: Tok::Number(n),
                at,
            }),
            Err(_) => Err(LexError {
                message: format!("{text} is not a number I can read"),
                at,
            }),
        }
    }

    fn string(&mut self) -> Result<Token, LexError> {
        let at = self.here();
        self.bump(); // opening quote
        // Bytes, decoded once at the end: a character such as é is two bytes in the source,
        // and turning each byte into a character on its own produced "Ã©".
        let mut out = Vec::new();
        loop {
            match self.peek() {
                0 => {
                    return Err(LexError {
                        message: "this string is never closed".into(),
                        at,
                    });
                }
                b'"' => {
                    self.bump();
                    let text = String::from_utf8(out)
                        .expect("the source is UTF-8 and every escape is ASCII");
                    return Ok(Token {
                        tok: Tok::Str(text),
                        at,
                    });
                }
                b'\\' => {
                    self.bump();
                    let escape = self.bump();
                    out.push(match escape {
                        b'n' => b'\n',
                        b't' => b'\t',
                        b'r' => b'\r',
                        b'\\' => b'\\',
                        b'"' => b'"',
                        other => {
                            return Err(LexError {
                                message: format!("\\{} is not an escape I know", other as char),
                                at: self.here(),
                            });
                        }
                    });
                }
                _ => {
                    let c = self.bump();
                    out.push(c);
                }
            }
        }
    }

    fn name(&mut self) -> Token {
        let at = self.here();
        let start = self.pos;
        while self.peek().is_ascii_alphanumeric() || self.peek() == b'_' {
            self.bump();
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let tok = match text {
            "let" => Tok::Let,
            "fn" => Tok::Fn,
            "return" => Tok::Return,
            "if" => Tok::If,
            "else" => Tok::Else,
            "while" => Tok::While,
            "for" => Tok::For,
            "in" => Tok::In,
            "true" => Tok::True,
            "false" => Tok::False,
            "nil" => Tok::Nil,
            "and" => Tok::And,
            "or" => Tok::Or,
            "not" => Tok::Not,
            other => Tok::Name(other.to_string()),
        };
        Token { tok, at }
    }

    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_blank();
        let at = self.here();
        let c = self.peek();
        if c == 0 {
            return Ok(Token { tok: Tok::Eof, at });
        }
        if c.is_ascii_digit() {
            return self.number();
        }
        if c == b'"' {
            return self.string();
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            return Ok(self.name());
        }
        self.bump();
        // Two-character operators are checked before the one-character ones they start with.
        let tok = match c {
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'[' => Tok::LBracket,
            b']' => Tok::RBracket,
            b',' => Tok::Comma,
            b';' => Tok::Semicolon,
            b':' => Tok::Colon,
            b'.' => Tok::Dot,
            b'+' => Tok::Plus,
            b'-' => Tok::Minus,
            b'*' => Tok::Star,
            b'/' => Tok::Slash,
            b'%' => Tok::Percent,
            b'=' if self.peek() == b'=' => {
                self.bump();
                Tok::Eq
            }
            b'=' => Tok::Assign,
            b'!' if self.peek() == b'=' => {
                self.bump();
                Tok::NotEq
            }
            b'<' if self.peek() == b'=' => {
                self.bump();
                Tok::LessEq
            }
            b'<' => Tok::Less,
            b'>' if self.peek() == b'=' => {
                self.bump();
                Tok::GreaterEq
            }
            b'>' => Tok::Greater,
            other => {
                return Err(LexError {
                    message: format!("I do not know what to do with {:?}", other as char),
                    at,
                });
            }
        };
        Ok(Token { tok, at })
    }
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(src);
    let mut out = Vec::new();
    loop {
        let token = lexer.next_token()?;
        let done = token.tok == Tok::Eof;
        out.push(token);
        if done {
            return Ok(out);
        }
    }
}
