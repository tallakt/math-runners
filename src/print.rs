//! Printing a neutral expression in a system's syntax. Each adapter is a
//! `Dialect`: its numbers, symbols and constants, its functions and its
//! operators; the printer puts in the parentheses.

use crate::expr::{Expr, Nested, Op, Rel};

/// The runner has no way to give the system this: the problem is written
/// as `unsupported`, with the reason as its message.
#[derive(Debug, Clone, PartialEq)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Out = Result<String, Unsupported>;

pub fn unsupported<T>(why: impl Into<String>) -> Result<T, Unsupported> {
    Err(Unsupported(why.into()))
}

// The precedences, from the loosest
const OR: u8 = 1;
const AND: u8 = 2;
const NOT: u8 = 3;
const REL: u8 = 4;
const ADD: u8 = 5;
const MUL: u8 = 6;
const NEG: u8 = 7;
const POW: u8 = 8;
const ATOM: u8 = 9;

pub trait Dialect {
    /// The system's name, for the messages.
    fn system(&self) -> &'static str;

    fn int(&self, n: &str) -> String {
        n.to_string()
    }

    fn dec(&self, d: &str) -> String {
        d.to_string()
    }

    /// A symbol, or a constant of the neutral syntax.
    fn symbol(&self, name: &str) -> Out;

    /// `-infinity`, where the system writes it as one word.
    fn minus_infinity(&self) -> Option<String> {
        None
    }

    /// A call of a function of the neutral syntax, or of an unknown one.
    fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out;

    /// Whether the dialect prints a system's answers back into the neutral syntax, where an
    /// unevaluated `integrate(f, x)` or a `sum` is an answer and no nested form of a problem.
    fn prints_answers(&self) -> bool {
        false
    }

    /// A nested form (an integral, a sum, a product, a limit) inside a problem's expression, as
    /// the system's own form that works it out: `body`, the variable (a symbol), and the bounds
    /// `[a, b]` of an integral, a sum or a product, or the point `[p]` of a limit.
    fn nested(&self, _p: &Printer, kind: Nested, _body: &Expr, _var: &Expr, _rest: &[Expr]) -> Out {
        unsupported(format!("{}: no form for a nested {kind:?}", self.system()))
    }

    /// `y'(0)`, the derivative of an unknown function at a point.
    fn prime(&self, _p: &Printer, name: &str, _order: usize, _args: &[Expr]) -> Out {
        unsupported(format!(
            "{}: no form for the derivative {name}'",
            self.system()
        ))
    }

    fn list(&self, items: Vec<String>) -> String {
        format!("[{}]", items.join(", "))
    }

    fn pow_op(&self) -> &'static str {
        "^"
    }

    /// A matrix literal of the formats, a list of lists, by rows: the system's matrix where a list
    /// of lists is not one.
    fn matrix(&self, rows: &[Vec<String>]) -> Out {
        Ok(self.list(rows.iter().map(|r| self.list(r.clone())).collect()))
    }

    /// A list that is a factor of a matrix product, a column vector.
    fn vector(&self, items: Vec<String>) -> String {
        self.list(items)
    }

    /// The product of two matrices, or of a matrix and a column vector, where `*` is not it.
    fn matmul(&self, a: &str, b: &str) -> String {
        format!("{a}*{b}")
    }

    /// A matrix to a power (the inverse is `-1`), where `^` is not it.
    fn matpow(&self, a: &str, b: &str) -> String {
        format!("{a}{}{b}", self.pow_op())
    }

    /// A relation, `None` for the infix form `a op b` with `rel_op`.
    fn rel(&self, _r: Rel, _a: &str, _b: &str) -> Option<String> {
        None
    }

    fn rel_op(&self, r: Rel) -> &'static str {
        r.neutral()
    }

    /// `and`, `or`, `not` as calls, where the system has no operators.
    fn logic_call(&self, _op: &str, _args: &[String]) -> Option<String> {
        None
    }

    fn and_op(&self) -> &'static str {
        "and"
    }

    fn or_op(&self) -> &'static str {
        "or"
    }

    fn not_op(&self) -> &'static str {
        "not "
    }
}

pub struct Printer<'a> {
    pub d: &'a dyn Dialect,
}

impl<'a> Printer<'a> {
    pub fn new(d: &'a dyn Dialect) -> Self {
        Printer { d }
    }

    pub fn print(&self, e: &Expr) -> Out {
        Ok(self.prec(e)?.0)
    }

    /// The arguments, printed, each at the top level.
    pub fn args(&self, args: &[Expr]) -> Result<Vec<String>, Unsupported> {
        args.iter().map(|a| self.print(a)).collect()
    }

    /// Printed as an operand: parenthesized unless an atom.
    pub fn atom(&self, e: &Expr) -> Out {
        let (s, p) = self.prec(e)?;
        Ok(if p >= ATOM { s } else { format!("({s})") })
    }

    /// An operand of a matrix product: a list is a column vector, anything else an atom.
    fn factor(&self, e: &Expr) -> Out {
        match e {
            Expr::List(items) if e.matrix_cells().is_none() => Ok(self.d.vector(self.args(items)?)),
            e => self.atom(e),
        }
    }

    /// A left operand: parenthesized where it binds looser than `min`.
    fn at_least(&self, e: &Expr, min: u8) -> Out {
        let (s, p) = self.prec(e)?;
        Ok(if p < min { format!("({s})") } else { s })
    }

    /// A right operand: a negation is parenthesized too.
    fn operand(&self, e: &Expr, min: u8) -> Out {
        let (s, p) = self.prec(e)?;
        Ok(if p < min || p == NEG {
            format!("({s})")
        } else {
            s
        })
    }

    fn prec(&self, e: &Expr) -> Result<(String, u8), Unsupported> {
        let d = self.d;
        Ok(match e {
            Expr::Int(n) => (d.int(n), ATOM),
            Expr::Dec(x) => (d.dec(x), ATOM),
            Expr::Sym(s) => (d.symbol(s)?, ATOM),
            Expr::Call(name, args) => match if d.prints_answers() {
                None
            } else {
                Nested::of(name, args).map_err(Unsupported)?
            } {
                Some(kind) => (d.nested(self, kind, &args[0], &args[1], &args[2..])?, ATOM),
                None => (d.call(self, name, args)?, ATOM),
            },
            Expr::Prime(name, k, args) => (d.prime(self, name, *k, args)?, ATOM),
            Expr::List(items) => match e.matrix_cells() {
                Some(rows) => {
                    let rows = rows
                        .iter()
                        .map(|r| self.args(r))
                        .collect::<Result<Vec<_>, _>>()?;
                    (d.matrix(&rows)?, ATOM)
                }
                None => (d.list(self.args(items)?), ATOM),
            },
            Expr::Neg(a) => {
                if let (Expr::Sym(s), Some(mi)) = (a.as_ref(), d.minus_infinity()) {
                    if s == "infinity" {
                        return Ok((mi, ATOM));
                    }
                }
                let (s, p) = self.prec(a)?;
                let s = if p <= NEG { format!("({s})") } else { s };
                (format!("-{s}"), NEG)
            }
            Expr::Bin(op, a, b) => match op {
                Op::Add => (
                    format!("{} + {}", self.at_least(a, ADD)?, self.operand(b, ADD)?),
                    ADD,
                ),
                Op::Sub => (
                    format!("{} - {}", self.at_least(a, ADD)?, self.operand(b, MUL)?),
                    ADD,
                ),
                // a matrix times a matrix or a list (a column), a list times a matrix
                Op::Mul if (a.has_matrix() || b.has_matrix()) && !is_scalar(a, b) => {
                    (d.matmul(&self.factor(a)?, &self.factor(b)?), MUL)
                }
                Op::Mul => (
                    format!("{}*{}", self.at_least(a, MUL)?, self.operand(b, NEG)?),
                    MUL,
                ),
                Op::Div => (
                    format!("{}/{}", self.at_least(a, MUL)?, self.operand(b, NEG)?),
                    MUL,
                ),
                Op::Pow if a.has_matrix() => (d.matpow(&self.atom(a)?, &self.atom(b)?), POW),
                Op::Pow => (
                    format!("{}{}{}", self.atom(a)?, d.pow_op(), self.atom(b)?),
                    POW,
                ),
            },
            Expr::Rel(r, a, b) => {
                let (sa, sb) = (self.at_least(a, ADD)?, self.at_least(b, ADD)?);
                match d.rel(*r, &sa, &sb) {
                    Some(s) => (s, ATOM),
                    None => (format!("{sa} {} {sb}", d.rel_op(*r)), REL),
                }
            }
            Expr::Not(a) => {
                if let Some(s) = d.logic_call("not", &[self.print(a)?]) {
                    (s, ATOM)
                } else {
                    (format!("{}{}", d.not_op(), self.operand(a, REL)?), NOT)
                }
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let (name, op, level) = if matches!(e, Expr::And(..)) {
                    ("and", d.and_op(), AND)
                } else {
                    ("or", d.or_op(), OR)
                };
                let (sa, sb) = (self.print(a)?, self.print(b)?);
                if let Some(s) = d.logic_call(name, &[sa, sb]) {
                    (s, ATOM)
                } else {
                    let sa = self.operand(a, level)?;
                    let sb = self.operand(b, level + 1)?;
                    (format!("{sa} {op} {sb}"), level)
                }
            }
        })
    }
}

/// Whether one of the factors of a product with a matrix in it is a scalar, so that the product
/// is the matrix scaled: neither is a list or a matrix.
fn is_scalar(a: &Expr, b: &Expr) -> bool {
    let plain = |e: &Expr| !e.has_matrix() && !matches!(e, Expr::List(_));
    plain(a) || plain(b)
}

/// Whether an expression is `infinity` or `-infinity`.
pub fn is_infinite(e: &Expr) -> bool {
    matches!(e, Expr::Sym(s) if s == "infinity")
        || matches!(e, Expr::Neg(a) if a.as_sym() == Some("infinity"))
}

/// The arguments of a neutral `subs(f, x = a)`: the expression, the
/// variable and the value.
pub fn subs_args(args: &[Expr]) -> Result<(Expr, String, Expr), Unsupported> {
    match args {
        [e, Expr::Rel(Rel::Eq, v, a)] => match v.as_ref() {
            Expr::Sym(v) => Ok((e.clone(), v.clone(), a.as_ref().clone())),
            _ => unsupported("subs: the variable is not a symbol"),
        },
        _ => unsupported("subs is not of the form subs(f, x = a)"),
    }
}

/// The arguments of a neutral `diff(f, x, 2)` or `diff(u, x, t)`: the
/// expression and the variables, each with its order.
pub fn diff_args(args: &[Expr]) -> Result<(Expr, Vec<(String, usize)>), Unsupported> {
    let Some((f, rest)) = args.split_first() else {
        return unsupported("diff without arguments");
    };
    let mut vars: Vec<(String, usize)> = Vec::new();
    for a in rest {
        match a {
            Expr::Sym(v) => vars.push((v.clone(), 1)),
            Expr::Int(_) => match vars.last_mut() {
                Some(last) if last.1 == 1 => last.1 = a.as_int().unwrap_or(1) as usize,
                _ => return unsupported("diff: an order without its variable"),
            },
            _ => return unsupported("diff: a variable is not a symbol"),
        }
    }
    if vars.is_empty() {
        return unsupported("diff without a variable");
    }
    Ok((f.clone(), vars))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::parse;

    struct Plain;

    impl Dialect for Plain {
        fn system(&self) -> &'static str {
            "plain"
        }
        fn symbol(&self, name: &str) -> Out {
            Ok(name.to_string())
        }
        fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out {
            Ok(format!("{name}({})", p.args(args)?.join(", ")))
        }
    }

    fn pp(s: &str) -> String {
        Printer::new(&Plain).print(&parse(s).unwrap()).unwrap()
    }

    #[test]
    fn parentheses() {
        assert_eq!(pp("-x^2"), "-x^2");
        assert_eq!(pp("(-x)^2"), "(-x)^2");
        assert_eq!(pp("a^b^c"), "a^(b^c)");
        assert_eq!(pp("(a^b)^c"), "(a^b)^c");
        assert_eq!(pp("a - (b - c)"), "a - (b - c)");
        assert_eq!(pp("a - b - c"), "a - b - c");
        assert_eq!(pp("a/(b*c)"), "a/(b*c)");
        assert_eq!(pp("a*-b"), "a*(-b)");
        assert_eq!(pp("a + -b"), "a + (-b)");
        assert_eq!(pp("-a*b"), "-a*b");
        assert_eq!(pp("-(a + b)"), "-(a + b)");
        assert_eq!(pp("2^-x"), "2^(-x)");
        assert_eq!(pp("x < -1 or x > 1"), "x < -1 or x > 1");
        assert_eq!(pp("not (a and b)"), "not (a and b)");
        assert_eq!(pp("exp(x^2)/(1 + x)"), "exp(x^2)/(1 + x)");
    }
}
