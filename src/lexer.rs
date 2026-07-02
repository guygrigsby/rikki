use crate::diag::Diag;
use crate::token::{Spanned, Token};

pub fn lex(src: &str) -> Result<Vec<Spanned<Token>>, Diag> {
    Lexer::new(src).run()
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: u32,
    col: u32,
    out: Vec<Spanned<Token>>,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        // Executable scripts: a `#!...` first line is the interpreter's,
        // not ours. Skip to its newline (kept, so line numbers stay true).
        let src = if src.starts_with("#!") {
            &src[src.find('\n').unwrap_or(src.len())..]
        } else {
            src
        };
        Lexer { chars: src.chars().peekable(), line: 1, col: 1, out: vec![] }
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn push(&mut self, tok: Token, line: u32, col: u32) {
        self.out.push(Spanned { node: tok, line, col });
    }

    fn err(&self, msg: &str, line: u32, col: u32) -> Diag {
        Diag { msg: msg.into(), line, col }
    }

    fn run(mut self) -> Result<Vec<Spanned<Token>>, Diag> {
        while let Some(&c) = self.chars.peek() {
            let (line, col) = (self.line, self.col);
            match c {
                ' ' | '\t' | '\r' => {
                    self.bump();
                }
                '\n' => {
                    self.bump();
                    if !matches!(self.out.last().map(|s| &s.node), None | Some(Token::Newline)) {
                        self.push(Token::Newline, line, col);
                    }
                }
                '/' => {
                    self.bump();
                    if self.chars.peek() == Some(&'/') {
                        while let Some(&n) = self.chars.peek() {
                            if n == '\n' {
                                break;
                            }
                            self.bump();
                        }
                    } else {
                        self.push(Token::Slash, line, col);
                    }
                }
                '"' => {
                    self.bump();
                    let mut s = String::new();
                    loop {
                        match self.bump() {
                            None | Some('\n') => {
                                return Err(self.err("unterminated string", line, col))
                            }
                            Some('"') => break,
                            Some('\\') => match self.bump() {
                                Some('n') => s.push('\n'),
                                Some('t') => s.push('\t'),
                                Some('"') => s.push('"'),
                                Some('\\') => s.push('\\'),
                                _ => return Err(self.err("bad escape", self.line, self.col)),
                            },
                            Some(ch) => s.push(ch),
                        }
                    }
                    self.push(Token::Str(s), line, col);
                }
                c if c.is_ascii_digit() => {
                    let mut n = String::new();
                    while let Some(&d) = self.chars.peek() {
                        if d.is_ascii_digit() {
                            n.push(d);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    // float only when '.' is followed by a digit (keeps xs[0].foo intact)
                    let mut is_float = false;
                    if self.chars.peek() == Some(&'.') {
                        let mut ahead = self.chars.clone();
                        ahead.next();
                        if ahead.peek().is_some_and(|d| d.is_ascii_digit()) {
                            is_float = true;
                            n.push('.');
                            self.bump();
                            while let Some(&d) = self.chars.peek() {
                                if d.is_ascii_digit() {
                                    n.push(d);
                                    self.bump();
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    if is_float {
                        let v = n.parse().map_err(|_| self.err("bad float", line, col))?;
                        self.push(Token::Float(v), line, col);
                    } else {
                        let v = n.parse().map_err(|_| self.err("int too large", line, col))?;
                        self.push(Token::Int(v), line, col);
                    }
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    let mut w = String::new();
                    while let Some(&a) = self.chars.peek() {
                        if a.is_ascii_alphanumeric() || a == '_' {
                            w.push(a);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let tok = match w.as_str() {
                        "fn" => Token::Fn,
                        "struct" => Token::Struct,
                        "import" => Token::Import,
                        "py" => Token::Py,
                        "return" => Token::Return,
                        "if" => Token::If,
                        "else" => Token::Else,
                        "for" => Token::For,
                        "in" => Token::In,
                        "break" => Token::Break,
                        "continue" => Token::Continue,
                        "check" => Token::Check,
                        "none" => Token::None_,
                        "true" => Token::True,
                        "false" => Token::False,
                        _ => Token::Ident(w),
                    };
                    self.push(tok, line, col);
                }
                _ => {
                    self.bump();
                    let two = |l: &mut Self, second: char, yes: Token, no: Token| {
                        if l.chars.peek() == Some(&second) {
                            l.bump();
                            yes
                        } else {
                            no
                        }
                    };
                    let tok = match c {
                        '(' => Token::LParen,
                        ')' => Token::RParen,
                        '{' => Token::LBrace,
                        '}' => Token::RBrace,
                        '[' => Token::LBracket,
                        ']' => Token::RBracket,
                        ',' => Token::Comma,
                        '.' => Token::Dot,
                        '+' => Token::Plus,
                        '-' => Token::Minus,
                        '*' => Token::Star,
                        '%' => Token::Percent,
                        '?' => Token::Question,
                        ':' => two(&mut self, '=', Token::ColonEq, Token::Colon),
                        '=' => two(&mut self, '=', Token::EqEq, Token::Eq),
                        '<' => two(&mut self, '=', Token::LtEq, Token::Lt),
                        '>' => two(&mut self, '=', Token::GtEq, Token::Gt),
                        '!' => two(&mut self, '=', Token::NotEq, Token::Bang),
                        '&' => {
                            if self.chars.peek() == Some(&'&') {
                                self.bump();
                                Token::AndAnd
                            } else {
                                return Err(self.err("expected &&", line, col));
                            }
                        }
                        '|' => {
                            if self.chars.peek() == Some(&'|') {
                                self.bump();
                                Token::OrOr
                            } else {
                                return Err(self.err("expected ||", line, col));
                            }
                        }
                        other => {
                            return Err(self.err(&format!("unexpected character {other:?}"), line, col))
                        }
                    };
                    self.push(tok, line, col);
                }
            }
        }
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Token::*;

    fn toks(src: &str) -> Vec<Token> {
        lex(src).unwrap().into_iter().map(|s| s.node).collect()
    }

    #[test]
    fn keywords_vs_idents() {
        assert_eq!(
            toks("fn checker check forx for py"),
            vec![
                Fn,
                Ident("checker".into()),
                Check,
                Ident("forx".into()),
                For,
                Py
            ]
        );
    }

    #[test]
    fn colon_forms() {
        assert_eq!(
            toks("a := b : c = d"),
            vec![
                Ident("a".into()),
                ColonEq,
                Ident("b".into()),
                Colon,
                Ident("c".into()),
                Eq,
                Ident("d".into())
            ]
        );
    }

    #[test]
    fn string_escapes() {
        assert_eq!(toks(r#""a\nb\t\"q\"\\""#), vec![Str("a\nb\t\"q\"\\".into())]);
    }

    #[test]
    fn unterminated_string_has_position() {
        let e = lex("x := \"oops").unwrap_err();
        assert!(e.msg.contains("unterminated"));
        assert_eq!((e.line, e.col), (1, 6));
    }

    #[test]
    fn numbers() {
        assert_eq!(toks("1 2.5 3.0"), vec![Int(1), Float(2.5), Float(3.0)]);
        // dot not followed by digit stays a method call, not a float
        assert_eq!(
            toks("1.abs"),
            vec![Int(1), Dot, Ident("abs".into())]
        );
    }

    #[test]
    fn slice_colon() {
        assert_eq!(
            toks("xs[a:b]"),
            vec![
                Ident("xs".into()),
                LBracket,
                Ident("a".into()),
                Colon,
                Ident("b".into()),
                RBracket
            ]
        );
    }

    #[test]
    fn comments_and_newlines() {
        assert_eq!(
            toks("a // hi\n\n\nb"),
            vec![Ident("a".into()), Newline, Ident("b".into())]
        );
        // leading newlines produce nothing
        assert_eq!(toks("\n\na"), vec![Ident("a".into())]);
    }

    #[test]
    fn shebang_skipped_at_file_start() {
        assert_eq!(
            toks("#!/usr/bin/env mg\nx := 1"),
            vec![Ident("x".into()), ColonEq, Int(1)]
        );
        // shebang line still counts for line numbers
        let t = lex("#!/bin/mg\nx := 1").unwrap();
        assert_eq!(t[0].line, 2);
        // whole file is just a shebang: no tokens
        assert_eq!(toks("#!/bin/mg"), vec![]);
        // only at file start; '#' later is still an error
        assert!(lex("x := 1\n#!/bin/mg").is_err());
    }

    #[test]
    fn operators() {
        assert_eq!(
            toks("== != <= >= && || ! ? %"),
            vec![EqEq, NotEq, LtEq, GtEq, AndAnd, OrOr, Bang, Question, Percent]
        );
    }
}
