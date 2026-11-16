//! The adapters, one for each system: how to start it, and the text that
//! gives it a problem.
//!
//! Every adapter speaks the same small protocol through the system's own
//! printing: the system prints, each line alone,
//!
//! ```text
//! @@MR-VERSION <the version, as the system gives it>   (at the start)
//! @@MR-READY <nonce>                                   (at the start)
//! @@MR-BEGIN <nonce>          the state is reset; the problem starts
//! @@MR-TIME <cpu> <wall>      the seconds of the work, timed inside
//! @@MR-ANSWER <nonce>         the answer, on the lines up to
//! @@MR-ANSWER-END <nonce>
//! @@MR-ERROR <message>        where it failed
//! @@MR-END <nonce>            the problem is done
//! ```
//!
//! Anything else the system prints between BEGIN and END is kept for the
//! message of an error.

use crate::print::Unsupported;
use crate::problem::Problem;
use crate::session::Spec;

pub mod fricas;
pub mod giac;
pub mod julia;
mod julia_read;
pub mod maple;
pub mod mathematica;
pub mod mathilda;
pub mod maxima;
pub mod sympy;

#[cfg(test)]
mod roundtrip;

pub trait Adapter: Send + Sync {
    /// `system.name` of the upload.
    fn name(&self) -> &'static str;

    /// The name on the command line.
    fn slug(&self) -> &'static str;

    /// The program that starts the system, where `--command` gives none.
    fn default_command(&self) -> &'static str;

    /// How to start the system from its program.
    fn spec(&self, program: &str) -> Spec;

    /// What to send first: set the session up, print the version and the
    /// ready line.
    fn start(&self, nonce: &str) -> String;

    /// One problem, with the state reset first.
    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported>;

    /// What to send to end the session.
    fn goodbye(&self) -> &'static str;

    /// The answer as printed, made one expression of the system's input
    /// syntax where the printing breaks it.
    fn clean_answer(&self, text: &str) -> String {
        text.trim().to_string()
    }

    /// The answer of the run file, from what the system printed between the
    /// answer's lines: the clean answer, or why the runner cannot read it
    /// (an `error`).
    fn read_answer(&self, text: &str) -> Result<String, String> {
        Ok(self.clean_answer(text))
    }

    /// The system's own chatter in the output (prompts, echoes), which is
    /// left out of the messages.
    fn clean_noise(&self, text: &str) -> String {
        text.trim().to_string()
    }

    /// The version as the system printed it, made the version alone where
    /// the system says more (its name, its copyright).
    fn clean_version(&self, text: &str) -> String {
        text.trim().to_string()
    }

    /// How long the system may take to start, its warm-up included.
    fn start_time(&self) -> std::time::Duration {
        std::time::Duration::from_secs(300)
    }

    /// Whether the adapter was tested against the system.
    fn tested(&self) -> bool {
        true
    }
}

pub fn all() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(sympy::SymPy),
        Box::new(maxima::Maxima),
        Box::new(fricas::FriCAS),
        Box::new(giac::Giac),
        Box::new(julia::Julia),
        Box::new(maple::Maple),
        Box::new(mathematica::Mathematica),
        Box::new(mathilda::Mathilda),
    ]
}

pub fn by_slug(slug: &str) -> Option<Box<dyn Adapter>> {
    all()
        .into_iter()
        .find(|a| a.slug() == slug.to_ascii_lowercase())
}

/// A string literal for systems that quote with `"` and escape with `\`.
pub fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The polynomial domains the runners know: the rationals (or the
/// integers, which factor alike), and the Gaussian rationals.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolyDomain {
    Rationals,
    Gaussian,
}

pub fn poly_domain(p: &Problem) -> Result<PolyDomain, Unsupported> {
    match p.text("domain").as_deref() {
        None | Some("") | Some("rationals") | Some("integers") | Some("QQ") | Some("ZZ") => {
            Ok(PolyDomain::Rationals)
        }
        Some("gaussian")
        | Some("gaussian_rationals")
        | Some("gaussian_integers")
        | Some("QQ_I") => Ok(PolyDomain::Gaussian),
        Some(d) => crate::print::unsupported(format!("the domain {d} is not known to the runner")),
    }
}

/// The monomial orders of `groebner`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MonomialOrder {
    Lex,
    Grlex,
    Grevlex,
}

pub fn monomial_order(p: &Problem) -> Result<MonomialOrder, Unsupported> {
    match p.text("order").as_deref() {
        None | Some("") | Some("lex") | Some("plex") => Ok(MonomialOrder::Lex),
        Some("grlex") | Some("deglex") => Ok(MonomialOrder::Grlex),
        Some("grevlex") | Some("degrevlex") | Some("tdeg") | Some("revlex") => {
            Ok(MonomialOrder::Grevlex)
        }
        Some(o) => crate::print::unsupported(format!("the monomial order {o} is not known")),
    }
}

/// The conditions of an ODE, read: each an equation of a value or a
/// derivative of an unknown function at a point.
#[derive(Clone, Debug)]
pub struct Condition {
    pub function: String,
    pub order: usize,
    pub at: Vec<crate::expr::Expr>,
    pub value: crate::expr::Expr,
}

pub fn conditions(p: &Problem, key: &str) -> Result<Vec<Condition>, Unsupported> {
    use crate::expr::{Expr, Rel};
    let funcs = p.unknown_functions()?;
    let mut out = Vec::new();
    for c in p.exprs(key)? {
        let Expr::Rel(Rel::Eq, lhs, rhs) = c else {
            return crate::print::unsupported("a condition is not an equation");
        };
        let (function, order, at) = match *lhs {
            Expr::Call(name, args) if funcs.iter().any(|f| f.0 == name) => (name, 0, args),
            Expr::Prime(name, k, args) if funcs.iter().any(|f| f.0 == name) => (name, k, args),
            // subs(diff(y(x), x, k), x = a): the formats' derivative at a point
            Expr::Call(name, args) if name == "subs" => {
                let (e, var, at) = crate::print::subs_args(&args)?;
                let (f, order) = match e {
                    Expr::Call(d, dargs) if d == "diff" => {
                        let (f, vars) = crate::print::diff_args(&dargs)?;
                        if vars.iter().any(|(v, _)| *v != var) {
                            return crate::print::unsupported(
                                "a condition on a derivative in another variable than the one set",
                            );
                        }
                        (f, vars.iter().map(|v| v.1).sum())
                    }
                    f => (f, 0),
                };
                match f {
                    Expr::Call(name, fargs)
                        if funcs.iter().any(|f| f.0 == name)
                            && fargs.len() == 1
                            && fargs[0].as_sym() == Some(var.as_str()) =>
                    {
                        (name, order, vec![at])
                    }
                    _ => {
                        return crate::print::unsupported(
                            "a condition is not of a function of one variable at a point",
                        );
                    }
                }
            }
            _ => {
                return crate::print::unsupported(
                    "a condition is not of the form y(a) = b or subs(diff(y(x), x), x = a) = b",
                );
            }
        };
        out.push(Condition {
            function,
            order,
            at,
            value: *rhs,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod nested_tests {
    use super::*;
    use serde_json::json;

    fn problem(expr: &str) -> Problem {
        Problem::from_json(&json!({
            "id": "t-1",
            "category": "special",
            "problem": {"op": "evaluate", "args": {"expr": expr}}
        }))
        .unwrap()
    }

    /// Every adapter writes a problem with nested forms: the forms printed, their variables
    /// given what the formats assume of them where the system takes assumptions.
    #[test]
    fn every_adapter_takes_nested_forms() {
        let p = problem(
            "sum(integrate(x^3, x, 0, k), k, 1, 3) + limit(sin(y)/y, y, 0) + product(j, j, 1, 3)",
        );
        for a in all() {
            let script = a
                .problem(&p, "n1")
                .unwrap_or_else(|e| panic!("{}: {e}", a.slug()));
            let assumed: &[&str] = match a.slug() {
                "sympy" => &[
                    "s_k = sp.Symbol('k', integer=True)",
                    "s_y = sp.Symbol('y', real=True)",
                    "s_x = sp.Symbol('x', real=True)",
                ],
                "mathematica" | "mathilda" => &[
                    "Element[k, Integers]",
                    "Element[y, Reals]",
                    "Element[x, Reals]",
                ],
                "maple" => &["k::integer", "y::real", "x::real"],
                _ => &[],
            };
            for want in assumed {
                assert!(script.contains(want), "{}: {want} in {script}", a.slug());
            }
        }
    }

    /// The indefinite integral, a wrong number of arguments and a variable that is no symbol are
    /// `unsupported` in every adapter, each with its reason.
    #[test]
    fn nested_forms_that_are_not_forms_are_refused() {
        for (expr, why) in [
            (
                "integrate(x^2, x)",
                "an indefinite integral inside a problem",
            ),
            ("sum(k, k, 1)", "sum of 3 arguments inside a problem"),
            ("limit(x, x, 0, 1)", "limit of 4 arguments inside a problem"),
            (
                "integrate(x, 2, 0, 1)",
                "the variable of integrate inside a problem is not a symbol",
            ),
        ] {
            let p = problem(expr);
            for a in all() {
                let e = a.problem(&p, "n1").unwrap_err();
                assert!(e.0.contains(why), "{}: {expr}: {e}", a.slug());
            }
        }
    }

    /// A matrix is a list of lists, a column a list among the factors of a matrix product: every
    /// adapter writes them.
    #[test]
    fn every_adapter_takes_matrices() {
        let p = problem("[[a, b], [c, d]] * [[1, 0], [0, 1]] * [x, y] + 2*[x, y]");
        for a in all() {
            a.problem(&p, "n1")
                .unwrap_or_else(|e| panic!("{}: {e}", a.slug()));
        }
    }

    /// An indexed quantity under a sum or a product is refused (the worker does it before the
    /// adapter): `b_j` over `j`, in a nested form and in a sum of the problem's own.
    #[test]
    fn indexed_quantities_are_refused() {
        let nested = problem("diff(sum(b_j^2/c_j, j, 1, K), b_k)");
        let why = nested.check_indexed().unwrap_err();
        assert!(
            why.0.contains("b_j") && why.0.contains("looks indexed by j"),
            "{why}"
        );
        let own = Problem::from_json(&json!({
            "id": "t-2",
            "category": "sum",
            "problem": {"op": "sum", "args": {"term": "y_k*x^k", "var": "k", "from": "1", "to": "n"}}
        }))
        .unwrap();
        assert!(own.check_indexed().is_err());
        // k_B is a constant of its own, a_i is not indexed by k
        assert!(
            problem("sum(k_B*j + a_i*j^2, j, 1, 3)")
                .check_indexed()
                .is_ok()
        );
    }
}
