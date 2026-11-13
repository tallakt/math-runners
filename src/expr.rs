//! The neutral expressions of the suite (section 3 of the formats): a parser
//! to a tree, which each adapter prints in its system's syntax.
//!
//! The grammar, from the loosest: `or`; `and`; `not`; the relations `=`,
//! `!=`, `<`, `<=`, `>`, `>=` (not chained); `+` and `-`; `*` and `/` (to the
//! left); unary `-`; `^` (to the right, its exponent may start with `-`);
//! calls `f(a, b)`, lists `[a, b]`, numbers and symbols.
//!
//! One extension: a name with primes, as `y'(0)` or `y''(0)`, is the
//! derivative of the unknown function `y` at a point, as
//! `subs(diff(y(x), x), x = 0)` is in the formats.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// An integer, as its digits.
    Int(String),
    /// A decimal, as `0.25`.
    Dec(String),
    /// A symbol or a constant (`pi`, `e`, `i`, `infinity`, ...).
    Sym(String),
    Call(String, Vec<Expr>),
    List(Vec<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Rel(Rel, Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    /// `y'(0)`: the `order`th derivative of the function `name` at the args.
    Prime(String, usize, Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rel {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Rel {
    pub fn neutral(self) -> &'static str {
        match self {
            Rel::Eq => "=",
            Rel::Ne => "!=",
            Rel::Lt => "<",
            Rel::Le => "<=",
            Rel::Gt => ">",
            Rel::Ge => ">=",
        }
    }
}

/// The constants of the neutral syntax, which are not symbols.
pub const CONSTANTS: &[&str] = &[
    "pi",
    "e",
    "i",
    "infinity",
    "euler_gamma",
    "catalan",
    "undefined",
];

pub fn is_constant(name: &str) -> bool {
    CONSTANTS.contains(&name)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub text: String,
    pub at: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot read `{}` at {}: {}",
            self.text, self.at, self.message
        )
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(String),
    Dec(String),
    Ident(String),
    Primes(usize),
    Punct(&'static str),
}

fn tokenize(text: &str) -> Result<Vec<(Tok, usize)>, ParseError> {
    let err = |at: usize, message: &str| ParseError {
        text: text.to_string(),
        at,
        message: message.to_string(),
    };
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                out.push((Tok::Dec(text[start..i].to_string()), start));
            } else {
                out.push((Tok::Num(text[start..i].to_string()), start));
            }
        } else if c == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            out.push((Tok::Dec(format!("0{}", &text[start..i])), start));
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((Tok::Ident(text[start..i].to_string()), start));
        } else if c == b'\'' {
            let start = i;
            while i < b.len() && b[i] == b'\'' {
                i += 1;
            }
            out.push((Tok::Primes(i - start), start));
        } else {
            let start = i;
            let two = if i + 1 < b.len() { &text[i..i + 2] } else { "" };
            let p: &'static str = match two {
                "!=" => "!=",
                "<=" => "<=",
                ">=" => ">=",
                "==" => "=",
                _ => match c {
                    b'+' => "+",
                    b'-' => "-",
                    b'*' => "*",
                    b'/' => "/",
                    b'^' => "^",
                    b'(' => "(",
                    b')' => ")",
                    b'[' => "[",
                    b']' => "]",
                    b',' => ",",
                    b'=' => "=",
                    b'<' => "<",
                    b'>' => ">",
                    _ => return Err(err(i, &format!("unexpected `{}`", c as char))),
                },
            };
            i += if p.len() == 2 || two == "==" { 2 } else { 1 };
            out.push((Tok::Punct(p), start));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    text: &'a str,
    toks: Vec<(Tok, usize)>,
    pos: usize,
}

impl Parser<'_> {
    fn err(&self, message: &str) -> ParseError {
        let at = self
            .toks
            .get(self.pos)
            .map(|t| t.1)
            .unwrap_or(self.text.len());
        ParseError {
            text: self.text.to_string(),
            at,
            message: message.to_string(),
        }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }

    fn punct(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }

    fn word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(q)) if q == w)
    }

    fn expect(&mut self, p: &str) -> Result<(), ParseError> {
        if self.punct(p) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected `{p}`")))
        }
    }

    fn or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.and()?;
        while self.word("or") {
            self.pos += 1;
            let right = self.and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.not()?;
        while self.word("and") {
            self.pos += 1;
            let right = self.not()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr, ParseError> {
        if self.word("not") {
            self.pos += 1;
            Ok(Expr::Not(Box::new(self.not()?)))
        } else {
            self.rel()
        }
    }

    fn rel(&mut self) -> Result<Expr, ParseError> {
        let left = self.sum()?;
        let rel = match self.peek() {
            Some(Tok::Punct("=")) => Rel::Eq,
            Some(Tok::Punct("!=")) => Rel::Ne,
            Some(Tok::Punct("<")) => Rel::Lt,
            Some(Tok::Punct("<=")) => Rel::Le,
            Some(Tok::Punct(">")) => Rel::Gt,
            Some(Tok::Punct(">=")) => Rel::Ge,
            _ => return Ok(left),
        };
        self.pos += 1;
        let right = self.sum()?;
        Ok(Expr::Rel(rel, Box::new(left), Box::new(right)))
    }

    fn sum(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.term()?;
        loop {
            let op = if self.punct("+") {
                Op::Add
            } else if self.punct("-") {
                Op::Sub
            } else {
                return Ok(left);
            };
            self.pos += 1;
            let right = self.term()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
    }

    fn term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.unary()?;
        loop {
            let op = if self.punct("*") {
                Op::Mul
            } else if self.punct("/") {
                Op::Div
            } else {
                return Ok(left);
            };
            self.pos += 1;
            let right = self.unary()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        if self.punct("-") {
            self.pos += 1;
            Ok(Expr::Neg(Box::new(self.unary()?)))
        } else if self.punct("+") {
            self.pos += 1;
            self.unary()
        } else {
            self.power()
        }
    }

    fn power(&mut self) -> Result<Expr, ParseError> {
        let base = self.primary()?;
        if self.punct("^") {
            self.pos += 1;
            let exponent = self.exponent()?;
            Ok(Expr::Bin(Op::Pow, Box::new(base), Box::new(exponent)))
        } else {
            Ok(base)
        }
    }

    fn exponent(&mut self) -> Result<Expr, ParseError> {
        if self.punct("-") {
            self.pos += 1;
            Ok(Expr::Neg(Box::new(self.exponent()?)))
        } else {
            self.power()
        }
    }

    fn args(&mut self, close: &str) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.punct(close) {
            self.pos += 1;
            return Ok(args);
        }
        loop {
            args.push(self.or()?);
            if self.punct(",") {
                self.pos += 1;
            } else {
                self.expect(close)?;
                return Ok(args);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self
            .peek()
            .cloned()
            .ok_or_else(|| self.err("unexpected end"))?;
        self.pos += 1;
        match tok {
            Tok::Num(n) => Ok(Expr::Int(n)),
            Tok::Dec(d) => Ok(Expr::Dec(d)),
            Tok::Ident(name) => {
                if let Some(Tok::Primes(k)) = self.peek().cloned() {
                    self.pos += 1;
                    self.expect("(")?;
                    let args = self.args(")")?;
                    return Ok(Expr::Prime(name, k, args));
                }
                if self.punct("(") {
                    self.pos += 1;
                    let args = self.args(")")?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Sym(name))
                }
            }
            Tok::Punct("(") => {
                let e = self.or()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Punct("[") => Ok(Expr::List(self.args("]")?)),
            _ => {
                self.pos -= 1;
                Err(self.err("unexpected token"))
            }
        }
    }
}

/// Parse a neutral expression.
pub fn parse(text: &str) -> Result<Expr, ParseError> {
    let toks = tokenize(text)?;
    let mut p = Parser { text, toks, pos: 0 };
    let e = p.or()?;
    if p.pos != p.toks.len() {
        return Err(p.err("unexpected text after the expression"));
    }
    Ok(e)
}

/// A nested form of the formats' section 3, written as a call inside a problem's expression: an
/// integral, a sum, a product or a limit, each binding its variable, which the system works out
/// to a value where the problem is given it. Never in an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nested {
    /// `integrate(f, x, a, b)`.
    Integral,
    /// `sum(term, k, a, b)`.
    Sum,
    /// `product(term, k, a, b)`.
    Product,
    /// `limit(f, x, p)`, from both sides.
    Limit,
}

impl Nested {
    /// What the formats assume of the variable of the form: real for an integral and a limit,
    /// an integer for a sum and a product.
    pub fn variable_is(self) -> &'static str {
        match self {
            Nested::Integral | Nested::Limit => "real",
            Nested::Sum | Nested::Product => "integer",
        }
    }

    /// The form a call is, where it is one: `Ok(None)` for any other call, an error for a call of
    /// the name of a form that is not one (`integrate(f, x)`: the indefinite integral is known
    /// only up to a constant, and has no value inside a problem).
    pub fn of(name: &str, args: &[Expr]) -> Result<Option<Nested>, String> {
        let (kind, n) = match name {
            "integrate" => (Nested::Integral, 4),
            "sum" => (Nested::Sum, 4),
            "product" => (Nested::Product, 4),
            "limit" => (Nested::Limit, 3),
            _ => return Ok(None),
        };
        if name == "integrate" && args.len() == 2 {
            return Err(
                "an indefinite integral inside a problem, which is known only up to a \
                        constant: a nested integral is integrate(f, x, a, b)"
                    .into(),
            );
        }
        if args.len() != n {
            return Err(format!(
                "{name} of {} arguments inside a problem: the nested forms are integrate(f, x, a, \
                 b), sum(term, k, a, b), product(term, k, a, b) and limit(f, x, p)",
                args.len()
            ));
        }
        match &args[1] {
            Expr::Sym(v) if !is_constant(v) => Ok(Some(kind)),
            _ => Err(format!(
                "the variable of {name} inside a problem is not a symbol"
            )),
        }
    }
}

impl Expr {
    /// The nested forms in the expression with their variables, in order.
    pub fn nested(&self, out: &mut Vec<(String, Nested)>) {
        match self {
            Expr::Call(name, args) => {
                if let Ok(Some(kind)) = Nested::of(name, args)
                    && let Some(v) = args[1].as_sym()
                    && !out.iter().any(|(w, k)| w == v && *k == kind)
                {
                    out.push((v.to_string(), kind));
                }
                for a in args {
                    a.nested(out);
                }
            }
            Expr::List(args) | Expr::Prime(_, _, args) => {
                for a in args {
                    a.nested(out);
                }
            }
            Expr::Neg(a) | Expr::Not(a) => a.nested(out),
            Expr::Bin(_, a, b) | Expr::Rel(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
                a.nested(out);
                b.nested(out);
            }
            Expr::Int(_) | Expr::Dec(_) | Expr::Sym(_) => {}
        }
    }

    /// Every symbol in the expression, constants left out, in order of
    /// first appearance. Function names are not symbols.
    pub fn symbols(&self, out: &mut Vec<String>) {
        match self {
            Expr::Sym(s) => {
                if !is_constant(s) && !out.contains(s) {
                    out.push(s.clone());
                }
            }
            Expr::Int(_) | Expr::Dec(_) => {}
            Expr::Call(_, args) | Expr::List(args) | Expr::Prime(_, _, args) => {
                for a in args {
                    a.symbols(out);
                }
            }
            Expr::Neg(a) | Expr::Not(a) => a.symbols(out),
            Expr::Bin(_, a, b) | Expr::Rel(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
                a.symbols(out);
                b.symbols(out);
            }
        }
    }

    pub fn as_sym(&self) -> Option<&str> {
        match self {
            Expr::Sym(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Expr::Int(n) => n.parse().ok(),
            Expr::Neg(a) => a.as_int().map(|n| -n),
            _ => None,
        }
    }

    /// The items of a list, or the expression alone.
    pub fn items(&self) -> Vec<Expr> {
        match self {
            Expr::List(v) => v.clone(),
            e => vec![e.clone()],
        }
    }

    /// Whether a symbol of `vars` is in the expression.
    pub fn has_any(&self, vars: &[String]) -> bool {
        let mut syms = Vec::new();
        self.symbols(&mut syms);
        syms.iter().any(|s| vars.contains(s))
    }

    /// Whether the expression, or both sides of the equation, is a
    /// polynomial in `vars`: sums, products and whole powers of them, with
    /// no function of them and no division by them.
    pub fn is_polynomial_in(&self, vars: &[String]) -> bool {
        match self {
            Expr::Int(_) | Expr::Dec(_) | Expr::Sym(_) => true,
            Expr::Rel(Rel::Eq, a, b) => a.is_polynomial_in(vars) && b.is_polynomial_in(vars),
            Expr::Neg(a) => a.is_polynomial_in(vars),
            Expr::Bin(Op::Add | Op::Sub | Op::Mul, a, b) => {
                a.is_polynomial_in(vars) && b.is_polynomial_in(vars)
            }
            Expr::Bin(Op::Div, a, b) => a.is_polynomial_in(vars) && !b.has_any(vars),
            Expr::Bin(Op::Pow, a, b) => {
                !self.has_any(vars)
                    || (a.is_polynomial_in(vars) && matches!(b.as_int(), Some(n) if n >= 0))
            }
            Expr::Call(..) => !self.has_any(vars),
            _ => false,
        }
    }

    /// Whether the expression has a relation, `and`, `or` or `not`.
    pub fn is_relation(&self) -> bool {
        matches!(
            self,
            Expr::Rel(..) | Expr::And(..) | Expr::Or(..) | Expr::Not(..)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Expr {
        parse(s).unwrap()
    }

    #[test]
    fn precedence() {
        assert_eq!(
            p("-x^2"),
            Expr::Neg(Box::new(Expr::Bin(
                Op::Pow,
                Box::new(Expr::Sym("x".into())),
                Box::new(Expr::Int("2".into()))
            )))
        );
        assert_eq!(p("a^b^c"), p("a^(b^c)"));
        assert_eq!(p("a/b/c"), p("(a/b)/c"));
        assert_eq!(p("a - b - c"), p("(a - b) - c"));
        assert_eq!(p("2^-x"), p("2^(-x)"));
        assert_eq!(p("x < -1 or x > 1"), p("(x < -1) or (x > 1)"));
        assert_eq!(p("not a = b and c"), p("(not (a = b)) and c"));
    }

    #[test]
    fn calls_lists_primes() {
        assert_eq!(
            p("[[1, 2], [3, 4]]"),
            Expr::List(vec![
                Expr::List(vec![Expr::Int("1".into()), Expr::Int("2".into())]),
                Expr::List(vec![Expr::Int("3".into()), Expr::Int("4".into())])
            ])
        );
        assert_eq!(
            p("y''(0)"),
            Expr::Prime("y".into(), 2, vec![Expr::Int("0".into())])
        );
        assert_eq!(p("0.25"), Expr::Dec("0.25".into()));
        let mut syms = vec![];
        p("diff(y(x), x, 2) + a1*y(x) = sin(pi*x)").symbols(&mut syms);
        assert_eq!(syms, vec!["x", "a1"]);
    }

    #[test]
    fn errors() {
        assert!(parse("2x").is_err());
        assert!(parse("f(").is_err());
        assert!(parse("a = b = c").is_err());
    }
}
