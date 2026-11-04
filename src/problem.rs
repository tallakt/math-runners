//! A problem of the suite (section 2 of the formats), and typed access to the
//! arguments of its task (section 4).

use serde_json::{Map, Value};

use crate::expr::{self, Expr};
use crate::print::{Unsupported, unsupported};

#[derive(Clone, Debug)]
pub struct Problem {
    pub id: String,
    #[allow(dead_code)]
    pub category: String,
    pub op: String,
    pub args: Map<String, Value>,
    pub quick: bool,
    pub calibration: bool,
}

impl Problem {
    pub fn from_json(v: &Value) -> Result<Problem, String> {
        let id = v["id"].as_str().ok_or("a line has no id")?.to_string();
        let category = v["category"]
            .as_str()
            .ok_or_else(|| format!("{id}: no category"))?
            .to_string();
        let op = v["problem"]["op"]
            .as_str()
            .ok_or_else(|| format!("{id}: no problem.op"))?
            .to_string();
        let args = v["problem"]["args"]
            .as_object()
            .cloned()
            .ok_or_else(|| format!("{id}: no problem.args"))?;
        Ok(Problem {
            id,
            category,
            op,
            args,
            quick: v["quick"].as_bool().unwrap_or(false),
            calibration: v["calibration"].as_bool().unwrap_or(false),
        })
    }

    pub fn has(&self, key: &str) -> bool {
        !matches!(self.args.get(key), None | Some(Value::Null))
    }

    /// An argument that is one expression.
    pub fn expr(&self, key: &str) -> Result<Expr, Unsupported> {
        match self.args.get(key) {
            Some(v) => value_expr(key, v),
            None => unsupported(format!("the argument {key} is missing")),
        }
    }

    /// An argument that is a list: a JSON array of expressions, or one
    /// expression that is a list, or one expression taken as a list of one.
    pub fn exprs(&self, key: &str) -> Result<Vec<Expr>, Unsupported> {
        match self.args.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => items.iter().map(|v| value_expr(key, v)).collect(),
            Some(v) => Ok(value_expr(key, v)?.items()),
        }
    }

    /// An argument that is a symbol, as `var`.
    pub fn symbol(&self, key: &str) -> Result<String, Unsupported> {
        match self.expr(key)? {
            Expr::Sym(s) => Ok(s),
            _ => unsupported(format!("the argument {key} is not a symbol")),
        }
    }

    pub fn symbols(&self, key: &str) -> Result<Vec<String>, Unsupported> {
        self.exprs(key)?
            .into_iter()
            .map(|e| match e {
                Expr::Sym(s) => Ok(s),
                _ => unsupported(format!("an item of {key} is not a symbol")),
            })
            .collect()
    }

    pub fn text(&self, key: &str) -> Option<String> {
        match self.args.get(key) {
            Some(Value::String(s)) => Some(s.trim().to_string()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    }

    pub fn int(&self, key: &str) -> Result<i64, Unsupported> {
        match self.args.get(key) {
            Some(Value::Number(n)) if n.is_i64() => Ok(n.as_i64().unwrap()),
            Some(Value::String(s)) => s
                .trim()
                .parse()
                .map_err(|_| Unsupported(format!("the argument {key} is not an integer"))),
            _ => unsupported(format!("the argument {key} is not an integer")),
        }
    }

    /// The limit's direction: `Some('+')`, `Some('-')`, or `None` for both
    /// sides (the argument left out, null, empty or `none`).
    pub fn direction(&self) -> Result<Option<char>, Unsupported> {
        match self.text("direction").as_deref() {
            None | Some("") | Some("none") | Some("both") => Ok(None),
            Some("+") | Some("right") | Some("plus") => Ok(Some('+')),
            Some("-") | Some("left") | Some("minus") => Ok(Some('-')),
            Some(d) => unsupported(format!("the direction {d} is not +, - or none")),
        }
    }

    /// The domain of `solve`: real or complex, complex where not given.
    pub fn real_domain(&self) -> Result<bool, Unsupported> {
        match self.text("domain").as_deref() {
            None | Some("complex") | Some("complexes") => Ok(false),
            Some("real") | Some("reals") => Ok(true),
            Some(d) => unsupported(format!("the domain {d} is not real or complex")),
        }
    }

    /// The transform's convention, which the runners know only as the
    /// formats' default.
    pub fn check_convention(&self) -> Result<(), Unsupported> {
        match self.text("convention").as_deref() {
            None | Some("") | Some("default") => Ok(()),
            Some(c) => unsupported(format!("the convention {c} is not known to the runner")),
        }
    }

    /// Every symbol of the arguments, in order.
    pub fn all_symbols(&self) -> Result<Vec<String>, Unsupported> {
        let mut out = Vec::new();
        for (key, v) in &self.args {
            if matches!(
                key.as_str(),
                "domain" | "direction" | "convention" | "order" | "statement"
            ) {
                continue;
            }
            for e in values_exprs(key, v)? {
                e.symbols(&mut out);
            }
        }
        // `element(n, integers)`: the set's name is not a symbol
        out.retain(|s| !SETS.contains(&s.as_str()));
        Ok(out)
    }

    /// The unknown functions: `function`, `functions`, as `y(x)`, with the
    /// names of their arguments.
    pub fn unknown_functions(&self) -> Result<Vec<(String, Vec<String>)>, Unsupported> {
        let mut out = Vec::new();
        for key in ["function", "functions"] {
            for e in self.exprs(key)? {
                match e {
                    Expr::Call(name, args) => {
                        let vars = args
                            .iter()
                            .map(|a| match a {
                                Expr::Sym(s) => Ok(s.clone()),
                                _ => unsupported("an unknown function's argument is not a symbol"),
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        out.push((name, vars));
                    }
                    _ => return unsupported(format!("{key} is not a function, as y(x)")),
                }
            }
        }
        Ok(out)
    }

    /// The assumptions: relations, and `element(n, set)`.
    pub fn assumptions(&self) -> Result<Vec<Assumption>, Unsupported> {
        let mut out = Vec::new();
        for e in self.exprs("assumptions")? {
            flatten_and(e, &mut out)?;
        }
        Ok(out)
    }
}

pub const SETS: &[&str] = &["naturals", "integers", "rationals", "reals", "complexes"];

#[derive(Clone, Debug, PartialEq)]
pub enum Assumption {
    /// A relation, as `a > 0`.
    Rel(Expr),
    /// `element(n, integers)`: the symbol, and the set.
    Element(String, String),
}

impl Assumption {
    /// `a > 0` as (`a`, `>`, `0`) where one side is a symbol and the other 0.
    pub fn sign(&self) -> Option<(String, &'static str)> {
        let Assumption::Rel(Expr::Rel(r, a, b)) = self else {
            return None;
        };
        use crate::expr::Rel::*;
        let zero = |e: &Expr| matches!(e, Expr::Int(n) if n == "0");
        match (a.as_ref(), b.as_ref()) {
            (Expr::Sym(s), z) if zero(z) => Some((
                s.clone(),
                match r {
                    Gt => "positive",
                    Ge => "nonnegative",
                    Lt => "negative",
                    Le => "nonpositive",
                    Ne => "nonzero",
                    Eq => "zero",
                },
            )),
            (z, Expr::Sym(s)) if zero(z) => Some((
                s.clone(),
                match r {
                    Gt => "negative",
                    Ge => "nonpositive",
                    Lt => "positive",
                    Le => "nonnegative",
                    Ne => "nonzero",
                    Eq => "zero",
                },
            )),
            _ => None,
        }
    }
}

fn flatten_and(e: Expr, out: &mut Vec<Assumption>) -> Result<(), Unsupported> {
    match e {
        Expr::And(a, b) => {
            flatten_and(*a, out)?;
            flatten_and(*b, out)
        }
        Expr::Call(name, args) if name == "element" && args.len() == 2 => {
            match (&args[0], &args[1]) {
                (Expr::Sym(v), Expr::Sym(set)) if SETS.contains(&set.as_str()) => {
                    out.push(Assumption::Element(v.clone(), set.clone()));
                    Ok(())
                }
                _ => unsupported("an assumption element(..) is not of a symbol and a set"),
            }
        }
        e @ Expr::Rel(..) => {
            out.push(Assumption::Rel(e));
            Ok(())
        }
        _ => unsupported("an assumption is not a relation or element(..)"),
    }
}

fn value_expr(key: &str, v: &Value) -> Result<Expr, Unsupported> {
    match v {
        Value::String(s) => expr::parse(s).map_err(|e| Unsupported(format!("{key}: {e}"))),
        Value::Number(n) => {
            let s = n.to_string();
            expr::parse(&s).map_err(|e| Unsupported(format!("{key}: {e}")))
        }
        Value::Array(items) => Ok(Expr::List(
            items
                .iter()
                .map(|i| value_expr(key, i))
                .collect::<Result<_, _>>()?,
        )),
        _ => unsupported(format!("the argument {key} is not an expression")),
    }
}

fn values_exprs(key: &str, v: &Value) -> Result<Vec<Expr>, Unsupported> {
    match v {
        Value::Array(items) => items.iter().map(|i| value_expr(key, i)).collect(),
        Value::String(_) | Value::Number(_) => Ok(vec![value_expr(key, v)?]),
        _ => Ok(Vec::new()),
    }
}
