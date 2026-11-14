//! Julia's answers read back into the neutral syntax (section 3 of the
//! formats). The driver prints the result as a tree (JSON, see
//! `julia_driver.jl`): each function by its module and name, as
//! `SpecialFunctions.besselj`, each number exactly. Each Julia function is
//! written as the neutral function that is the same function; one that has
//! none is not guessed at: the answer is then unreadable, an `error`.

use serde_json::Value;

use crate::expr::{Expr, Op, Rel};
use crate::print::{Dialect, Out, Printer};

/// The neutral syntax itself, as the printer writes it.
struct Neutral;

impl Dialect for Neutral {
    fn system(&self) -> &'static str {
        "neutral"
    }

    fn symbol(&self, name: &str) -> Out {
        Ok(name.to_string())
    }

    fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out {
        Ok(format!("{name}({})", p.args(args)?.join(", ")))
    }
}

/// The driver's tree of an answer, in the neutral syntax.
pub fn read_answer(text: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("the driver's tree is not JSON: {e}"))?;
    let e = node(&v)?;
    Printer::new(&Neutral).print(&e).map_err(|u| u.0)
}

fn int(n: &str) -> Expr {
    Expr::Int(n.to_string())
}

fn sym(s: &str) -> Expr {
    Expr::Sym(s.to_string())
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(name.to_string(), args)
}

fn bin(op: Op, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

fn neg(a: Expr) -> Expr {
    match a {
        Expr::Neg(x) => *x,
        a => Expr::Neg(Box::new(a)),
    }
}

/// An integer of Julia's digits, with its sign.
fn signed_int(s: &str) -> Result<Expr, String> {
    let (minus, digits) = match s.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("`{s}` is not an integer"));
    }
    let digits = digits.trim_start_matches('0');
    let e = int(if digits.is_empty() { "0" } else { digits });
    Ok(if minus { neg(e) } else { e })
}

/// A float as Julia prints it (`0.5`, `1.0e-5`, `Inf`), as a decimal of
/// the neutral syntax, with the same digits.
fn float(s: &str) -> Result<Expr, String> {
    match s {
        "Inf" => return Ok(sym("infinity")),
        "-Inf" => return Ok(neg(sym("infinity"))),
        "NaN" => return Err("NaN, which has no form in the formats".into()),
        _ => {}
    }
    let (minus, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s),
    };
    let (mantissa, exp) = match body.find(['e', 'E']) {
        Some(i) => (
            &body[..i],
            body[i + 1..]
                .trim_start_matches('+')
                .parse::<i64>()
                .map_err(|_| format!("`{s}` is not a float"))?,
        ),
        None => (body, 0),
    };
    let (whole, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() && frac.is_empty()
        || !whole.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit())
    {
        return Err(format!("`{s}` is not a float"));
    }
    // the digits, and where the point is among them
    let digits = format!("{whole}{frac}");
    let point = whole.len() as i64 + exp;
    let (w, f) = if point <= 0 {
        (
            "0".to_string(),
            format!("{}{digits}", "0".repeat((-point) as usize)),
        )
    } else if point as usize >= digits.len() {
        (
            format!("{digits}{}", "0".repeat(point as usize - digits.len())),
            String::new(),
        )
    } else {
        (
            digits[..point as usize].to_string(),
            digits[point as usize..].to_string(),
        )
    };
    let w = w.trim_start_matches('0');
    let f = f.trim_end_matches('0');
    let text = format!(
        "{}.{}",
        if w.is_empty() { "0" } else { w },
        if f.is_empty() { "0" } else { f }
    );
    let e = Expr::Dec(text);
    Ok(if minus { neg(e) } else { e })
}

/// A symbol's name: Julia's subscript digits (`C₁`) as digits; another
/// name must be one of the neutral syntax.
fn name(s: &str) -> Result<String, String> {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '₀'..='₉' => out.push(char::from(b'0' + (c as u32 - '₀' as u32) as u8)),
            c if c.is_ascii_alphanumeric() || c == '_' => out.push(c),
            _ => return Err(format!("the name `{s}` is not a name of the formats")),
        }
    }
    if !out.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Err(format!("the name `{s}` is not a name of the formats"));
    }
    Ok(out)
}

fn is_zero(e: &Expr) -> bool {
    match e {
        Expr::Int(n) => n == "0",
        Expr::Dec(d) => d.bytes().all(|b| b == b'0' || b == b'.'),
        Expr::Neg(a) => is_zero(a),
        _ => false,
    }
}

fn is_one(e: &Expr) -> bool {
    matches!(e, Expr::Int(n) if n == "1")
}

/// The expression without its leading minus, where it has one: `-c`,
/// `-2*x`, `-1/2*x` are `c`, `2*x`, `1/2*x`.
fn without_minus(e: &Expr) -> Option<Expr> {
    match e {
        Expr::Neg(c) => Some((**c).clone()),
        Expr::Bin(op @ (Op::Mul | Op::Div), a, b) => {
            without_minus(a).map(|a| {
                if is_one(&a) && *op == Op::Mul {
                    (**b).clone()
                } else {
                    bin(*op, a, (**b).clone())
                }
            })
        }
        _ => None,
    }
}

/// `a + b`, as `a - c` where b is `-c`.
fn add(a: Expr, b: Expr) -> Expr {
    match without_minus(&b) {
        Some(c) => bin(Op::Sub, a, c),
        None => bin(Op::Add, a, b),
    }
}

/// The product of the factors, a negative coefficient written as the minus
/// of the first: `-1*x*y` is `-x*y`, `-1/2*x` is `-1/2*x`.
fn product(factors: Vec<Expr>) -> Expr {
    let mut negative = false;
    let mut kept = Vec::new();
    for f in factors {
        match f {
            Expr::Neg(a) if matches!(*a, Expr::Int(_) | Expr::Dec(_)) || is_ratio(&a) => {
                negative = !negative;
                if !is_one(&a) {
                    kept.push(*a);
                }
            }
            f => kept.push(f),
        }
    }
    let mut it = kept.into_iter();
    let first = it.next().unwrap_or_else(|| int("1"));
    let first = if negative {
        match first {
            Expr::Bin(Op::Div, n, d) if matches!(*n, Expr::Int(_)) => {
                bin(Op::Div, neg(*n), *d)
            }
            f => neg(f),
        }
    } else {
        first
    };
    it.fold(first, |acc, f| bin(Op::Mul, acc, f))
}

fn is_ratio(e: &Expr) -> bool {
    matches!(e, Expr::Bin(Op::Div, a, b) if matches!(**a, Expr::Int(_)) && matches!(**b, Expr::Int(_)))
}

fn node(v: &Value) -> Result<Expr, String> {
    let a = v
        .as_array()
        .ok_or_else(|| format!("a node of the tree is not a list: {v}"))?;
    let tag = a.first().and_then(|t| t.as_str()).unwrap_or("");
    let text = |i: usize| -> Result<&str, String> {
        a.get(i)
            .and_then(|t| t.as_str())
            .ok_or_else(|| format!("the node {v} is cut short"))
    };
    let kid = |i: usize| -> Result<Expr, String> {
        node(a.get(i).ok_or_else(|| format!("the node {v} is cut short"))?)
    };
    let kids = |from: usize| -> Result<Vec<Expr>, String> {
        a.iter().skip(from).map(node).collect()
    };
    Ok(match tag {
        "i" => signed_int(text(1)?)?,
        "q" => {
            let (n, d) = (signed_int(text(1)?)?, signed_int(text(2)?)?);
            if is_one(&d) {
                n
            } else {
                bin(Op::Div, n, d)
            }
        }
        "f" => float(text(1)?)?,
        "c" => {
            let (re, im) = (kid(1)?, kid(2)?);
            let imag = if is_one(&im) {
                sym("i")
            } else {
                match im {
                    Expr::Neg(x) if is_one(&x) => neg(sym("i")),
                    Expr::Neg(x) => neg(bin(Op::Mul, *x, sym("i"))),
                    im => bin(Op::Mul, im, sym("i")),
                }
            };
            if is_zero(&re) { imag } else { add(re, imag) }
        }
        "k" => match text(1)? {
            "π" => sym("pi"),
            "ℯ" => sym("e"),
            "γ" => sym("euler_gamma"),
            "catalan" => sym("catalan"),
            k => return Err(format!("the constant {k} has no name in the formats")),
        },
        "b" => sym(text(1)?),
        "nothing" => sym("nothing"),
        "s" => sym(&name(text(1)?)?),
        "fn" => {
            let n = name(text(1)?)?;
            if crate::functions::NEUTRAL.contains(&n.as_str()) || crate::expr::is_constant(&n) {
                return Err(format!(
                    "Julia's symbolic function {n} has the name of a function of the formats"
                ));
            }
            call(&n, kids(2)?)
        }
        "d" => {
            let (var, order, f) = (kid(1)?, kid(2)?, kid(3)?);
            if var.as_sym().is_none() {
                return Err("a derivative in another than a symbol".into());
            }
            if is_one(&order) {
                call("diff", vec![f, var])
            } else if matches!(order, Expr::Int(_)) {
                call("diff", vec![f, var, order])
            } else {
                return Err("a derivative of an order that is not a whole number".into());
            }
        }
        "int" => {
            let (var, lo, hi, f) = (kid(1)?, kid(2)?, kid(3)?, kid(4)?);
            let var = match var {
                Expr::List(mut v) if v.len() == 1 => v.remove(0),
                v => v,
            };
            if var.as_sym().is_none() {
                return Err("an integral in another than a symbol".into());
            }
            call("integrate", vec![f, var, lo, hi])
        }
        "eq" => Expr::Rel(Rel::Eq, Box::new(kid(1)?), Box::new(kid(2)?)),
        "rel" => {
            let r = match text(1)? {
                "<" => Rel::Lt,
                "<=" | "≤" => Rel::Le,
                ">" => Rel::Gt,
                ">=" | "≥" => Rel::Ge,
                "==" => Rel::Eq,
                "!=" | "≠" => Rel::Ne,
                r => return Err(format!("the relation {r}")),
            };
            Expr::Rel(r, Box::new(kid(2)?), Box::new(kid(3)?))
        }
        "l" | "m" => Expr::List(kids(1)?),
        "call" => function(text(1)?, kids(2)?)?,
        t => return Err(format!("a node of the kind `{t}`")),
    })
}

/// A call of a Julia function, as the neutral function that is the same.
fn function(f: &str, mut args: Vec<Expr>) -> Result<Expr, String> {
    let n = args.len();
    let short = f.rsplit('.').next().unwrap_or(f);
    let module = &f[..f.len() - short.len()].trim_end_matches('.');
    let mut take = || args.remove(0);
    Ok(match (*module, short, n) {
        ("Base", "+", _) if n >= 1 => {
            let first = take();
            args.into_iter().fold(first, add)
        }
        ("Base", "*", _) if n >= 1 => product(args),
        ("Base", "-", 1) => neg(take()),
        ("Base", "-", 2) => {
            let a = take();
            bin(Op::Sub, a, take())
        }
        ("Base", "/" | "//", 2) => {
            let a = take();
            bin(Op::Div, a, take())
        }
        ("Base", "^", 2) => {
            let a = take();
            bin(Op::Pow, a, take())
        }
        ("Base", "inv", 1) => bin(Op::Div, int("1"), take()),
        (
            "Base",
            "exp" | "log" | "sqrt" | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "asin"
            | "acos" | "atan" | "acot" | "asec" | "acsc" | "sinh" | "cosh" | "tanh" | "coth"
            | "sech" | "csch" | "asinh" | "acosh" | "atanh" | "acoth" | "asech" | "acsch" | "abs"
            | "sign" | "floor",
            1,
        ) => call(short, args),
        ("Base", "log", 2) => call("log", args),
        ("Base", "log2", 1) => call("log", vec![int("2"), take()]),
        ("Base", "log10", 1) => call("log", vec![int("10"), take()]),
        ("Base", "log1p", 1) => call("log", vec![add(int("1"), take())]),
        ("Base", "expm1", 1) => bin(Op::Sub, call("exp", args), int("1")),
        ("Base", "exp2", 1) => bin(Op::Pow, int("2"), take()),
        ("Base", "exp10", 1) => bin(Op::Pow, int("10"), take()),
        ("Base", "atan", 2) => call("atan2", args),
        ("Base", "ceil", 1) => call("ceiling", args),
        ("Base", "real", 1) => call("re", args),
        ("Base", "imag", 1) => call("im", args),
        ("Base", "angle", 1) => call("arg", args),
        ("Base", "conj", 1) => call("conjugate", args),
        ("Base", "abs2", 1) => bin(Op::Pow, call("abs", args), int("2")),
        ("Base", "factorial", 1) | ("Base", "binomial", 2) => call(short, args),
        ("SpecialFunctions", "gamma", 1 | 2) => call("gamma", args),
        ("SpecialFunctions", "loggamma", 1) => call("log_gamma", args),
        ("SpecialFunctions", "digamma", 1) => call("digamma", args),
        ("SpecialFunctions", "polygamma", 2) => call("polygamma", args),
        ("SpecialFunctions", "beta", 2) => call("beta", args),
        ("SpecialFunctions", "erf" | "erfc" | "erfi", 1) => call(short, args),
        ("SpecialFunctions", "expinti", 1) => call("ei", args),
        ("SpecialFunctions", "expint", 1) => call("expint", vec![int("1"), take()]),
        ("SpecialFunctions", "expint", 2) => call("expint", args),
        ("SpecialFunctions", "sinint", 1) => call("si", args),
        ("SpecialFunctions", "cosint", 1) => call("ci", args),
        ("SpecialFunctions", "zeta", 1 | 2) => call("zeta", args),
        ("SpecialFunctions", "besselj", 2) => call("bessel_j", args),
        ("SpecialFunctions", "bessely", 2) => call("bessel_y", args),
        ("SpecialFunctions", "besseli", 2) => call("bessel_i", args),
        ("SpecialFunctions", "besselk", 2) => call("bessel_k", args),
        ("SpecialFunctions", "hankelh1", 2) => call("hankel_1", args),
        ("SpecialFunctions", "hankelh2", 2) => call("hankel_2", args),
        ("SpecialFunctions", "besselj0", 1) => call("bessel_j", vec![int("0"), take()]),
        ("SpecialFunctions", "besselj1", 1) => call("bessel_j", vec![int("1"), take()]),
        ("SpecialFunctions", "bessely0", 1) => call("bessel_y", vec![int("0"), take()]),
        ("SpecialFunctions", "bessely1", 1) => call("bessel_y", vec![int("1"), take()]),
        ("SpecialFunctions", "airyai", 1) => call("airy_ai", args),
        ("SpecialFunctions", "airybi", 1) => call("airy_bi", args),
        ("SpecialFunctions", "airyaiprime", 1) => call("airy_ai_prime", args),
        ("SpecialFunctions", "airybiprime", 1) => call("airy_bi_prime", args),
        ("SpecialFunctions", "ellipk", 1) => call("elliptic_k", args),
        ("SpecialFunctions", "ellipe", 1) => call("elliptic_e", args),
        _ => {
            return Err(format!(
                "Julia's function {f} of {n} arguments has no form in the formats"
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(tree: &str) -> String {
        read_answer(tree).unwrap()
    }

    #[test]
    fn numbers() {
        assert_eq!(read(r#"["i","12"]"#), "12");
        assert_eq!(read(r#"["i","-12"]"#), "-12");
        assert_eq!(read(r#"["q","-1","2"]"#), "-1/2");
        assert_eq!(read(r#"["q","3","1"]"#), "3");
        assert_eq!(read(r#"["f","0.5"]"#), "0.5");
        assert_eq!(read(r#"["f","1.0e-5"]"#), "0.00001");
        assert_eq!(read(r#"["f","-1.25e3"]"#), "-1250.0");
        assert_eq!(read(r#"["f","1.5e10"]"#), "15000000000.0");
        assert_eq!(read(r#"["f","Inf"]"#), "infinity");
        assert_eq!(read(r#"["f","-Inf"]"#), "-infinity");
        assert!(read_answer(r#"["f","NaN"]"#).is_err());
        assert_eq!(read(r#"["c",["i","0"],["i","1"]]"#), "i");
        assert_eq!(read(r#"["c",["i","2"],["i","-3"]]"#), "2 - 3*i");
        assert_eq!(read(r#"["c",["q","1","2"],["q","1","2"]]"#), "1/2 + 1/2*i");
        assert_eq!(read(r#"["k","π"]"#), "pi");
        assert_eq!(read(r#"["k","ℯ"]"#), "e");
        assert_eq!(read(r#"["k","γ"]"#), "euler_gamma");
        assert!(read_answer(r#"["k","φ"]"#).is_err());
    }

    #[test]
    fn sums_products_and_signs() {
        let x = r#"["s","x"]"#;
        let y = r#"["s","y"]"#;
        // x - y, as Julia's sum with -1*y
        assert_eq!(
            read(&format!(r#"["call","Base.+",{x},["call","Base.*",["i","-1"],{y}]]"#)),
            "x - y"
        );
        // 1 - 2*x + x^2/3
        assert_eq!(
            read(&format!(
                r#"["call","Base.+",["i","1"],["call","Base.*",["i","-2"],{x}],["call","Base.*",["q","1","3"],["call","Base.^",{x},["i","2"]]]]"#
            )),
            "1 - 2*x + 1/3*x^2"
        );
        // -(1/2)*x*y
        assert_eq!(
            read(&format!(r#"["call","Base.*",["q","-1","2"],{x},{y}]"#)),
            "-1/2*x*y"
        );
        assert_eq!(
            read(&format!(r#"["call","Base./",["call","Base.-",{x}],{y}]"#)),
            "-x/y"
        );
        assert_eq!(
            read(&format!(r#"["call","Base.^",{x},["q","1","2"]]"#)),
            "x^(1/2)"
        );
        assert_eq!(
            read(&format!(r#"["call","Base.^",["call","Base.+",{x},["i","1"]],["i","-2"]]"#)),
            "(x + 1)^(-2)"
        );
    }

    #[test]
    fn functions_by_their_neutral_names() {
        let x = r#"["s","x"]"#;
        assert_eq!(
            read(&format!(r#"["call","SpecialFunctions.besselj",["i","1"],{x}]"#)),
            "bessel_j(1, x)"
        );
        assert_eq!(
            read(&format!(r#"["call","SpecialFunctions.expint",{x}]"#)),
            "expint(1, x)"
        );
        assert_eq!(
            read(&format!(r#"["call","SpecialFunctions.loggamma",{x}]"#)),
            "log_gamma(x)"
        );
        assert_eq!(
            read(&format!(r#"["call","Base.atan",["s","y"],{x}]"#)),
            "atan2(y, x)"
        );
        assert_eq!(
            read(&format!(r#"["call","Base.log",["i","2"],{x}]"#)),
            "log(2, x)"
        );
        assert_eq!(
            read(&format!(r#"["call","Base.log10",{x}]"#)),
            "log(10, x)"
        );
        assert_eq!(read(&format!(r#"["call","Base.ceil",{x}]"#)), "ceiling(x)");
        assert_eq!(read(&format!(r#"["call","Base.conj",{x}]"#)), "conjugate(x)");
        // none is guessed at
        assert!(read_answer(&format!(r#"["call","Base.cbrt",{x}]"#)).is_err());
        assert!(read_answer(&format!(r#"["call","Base.ifelse",{x},{x},{x}]"#)).is_err());
        assert!(read_answer(&format!(r#"["call","SpecialFunctions.besselj",{x}]"#)).is_err());
    }

    #[test]
    fn functions_equations_and_lists() {
        let x = r#"["s","x"]"#;
        let y = format!(r#"["fn","y",{x}]"#);
        assert_eq!(
            read(&format!(
                r#"["l",["eq",{y},["call","Base.*",["s","C₁"],["call","Base.exp",{x}]]]]"#
            )),
            "[y(x) = C1*exp(x)]"
        );
        assert_eq!(
            read(&format!(r#"["d",{x},["i","2"],{y}]"#)),
            "diff(y(x), x, 2)"
        );
        assert_eq!(
            read(&format!(r#"["int",["l",{x}],["i","0"],["f","Inf"],{x}]"#)),
            "integrate(x, x, 0, infinity)"
        );
        assert_eq!(
            read(r#"["m",["l",["i","1"],["i","2"]],["l",["i","3"],["i","4"]]]"#),
            "[[1, 2], [3, 4]]"
        );
        assert_eq!(read(r#"["nothing"]"#), "nothing");
        assert_eq!(
            read(&format!(r#"["rel","<",{x},["i","1"]]"#)),
            "x < 1"
        );
        assert!(read_answer(r#"["fn","sin",["s","x"]]"#).is_err());
        assert!(read_answer(r#"["s","xˍt"]"#).is_err());
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        for tree in [
            r#"["call","Base.+",["s","x"],["call","Base.*",["i","-1"],["call","Base.^",["s","x"],["call","Base.+",["s","a"],["i","1"]]]]]"#,
            r#"["call","Base.*",["q","-3","4"],["call","Base.^",["call","Base.-",["s","x"]],["i","2"]]]"#,
            r#"["call","Base./",["i","1"],["call","Base.*",["s","a"],["s","b"]]]"#,
        ] {
            let text = read(tree);
            let e = crate::expr::parse(&text).unwrap();
            assert_eq!(Printer::new(&Neutral).print(&e).unwrap(), text, "{tree}");
        }
    }
}
