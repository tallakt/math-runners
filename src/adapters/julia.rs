//! Julia, with Symbolics.jl, driven by a small Julia program
//! (`julia_driver.jl`) kept running as a session, which runs the code this
//! adapter writes for each problem in a fresh module. The answer is read
//! from Symbolics' expression tree into the neutral syntax
//! (`julia_read.rs`).

use super::julia_read::read_answer;
use super::{Adapter, MonomialOrder, PolyDomain, conditions, monomial_order, poly_domain};
use crate::expr::{Expr, Op, Rel};
use crate::print::{Dialect, Out, Printer, Unsupported, diff_args, unsupported};
use crate::problem::Problem;
use crate::session::Spec;

pub struct Julia;

const DRIVER: &str = include_str!("julia_driver.jl");

/// Julia's shared environment of the pinned packages, `@math-run`.
pub const JULIA_ENVIRONMENT: &str = "math-run";

/// The Julia name of a symbol of the problem: Symbolics' variable is named
/// as in the problem, its binding in the problem's module has `s_` before
/// it, so that no name of the problem is one of Julia's.
fn var(name: &str) -> String {
    format!("s_{name}")
}

fn fun(name: &str) -> String {
    format!("f_{name}")
}

/// Whether the expression has no symbol: a number, a constant, or a
/// function of them.
fn is_constant(e: &Expr) -> bool {
    let mut syms = Vec::new();
    e.symbols(&mut syms);
    syms.is_empty() && !has_unknown_call(e)
}

fn has_unknown_call(e: &Expr) -> bool {
    match e {
        // the marks of `exact` are numbers
        Expr::Call(n, a) => {
            !(crate::functions::NEUTRAL.contains(&n.as_str()) || n.starts_with('\u{1}'))
                || a.iter().any(has_unknown_call)
        }
        Expr::Prime(..) => true,
        Expr::List(a) => a.iter().any(has_unknown_call),
        Expr::Neg(a) | Expr::Not(a) => has_unknown_call(a),
        Expr::Bin(_, a, b) | Expr::Rel(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
            has_unknown_call(a) || has_unknown_call(b)
        }
        _ => false,
    }
}

/// Whether the expression is a rational number: integers, and sums,
/// products, quotients and whole powers of them.
fn is_rational(e: &Expr) -> bool {
    match e {
        Expr::Int(_) => true,
        Expr::Neg(a) => is_rational(a),
        Expr::Bin(Op::Add | Op::Sub | Op::Mul | Op::Div, a, b) => is_rational(a) && is_rational(b),
        Expr::Bin(Op::Pow, a, b) => is_rational(a) && b.as_int().is_some(),
        _ => false,
    }
}

struct D {
    funcs: Vec<(String, Vec<String>)>,
    /// How an unknown function is written: called (`f_y(s_x)`), or as the
    /// plain symbol of the dependent variable (`f_y`), as
    /// `symbolic_solve_ode` takes it.
    plain_functions: bool,
}

impl Dialect for D {
    fn system(&self) -> &'static str {
        "Julia"
    }

    fn int(&self, n: &str) -> String {
        n.to_string()
    }

    fn symbol(&self, name: &str) -> Out {
        Ok(match name {
            "pi" => "Num(π)".into(),
            "e" => "Num(ℯ)".into(),
            "i" => "im".into(),
            "infinity" => "Inf".into(),
            "euler_gamma" => "Num(Base.MathConstants.eulergamma)".into(),
            "catalan" => "Num(Base.MathConstants.catalan)".into(),
            "undefined" => return unsupported("Julia has no undefined value of the formats'"),
            s => var(s),
        })
    }

    fn rel(&self, r: Rel, a: &str, b: &str) -> Option<String> {
        match r {
            Rel::Eq => Some(format!("{a} ~ {b}")),
            _ => None,
        }
    }

    fn rel_op(&self, r: Rel) -> &'static str {
        match r {
            Rel::Ne => "!=",
            r => r.neutral(),
        }
    }

    fn prime(&self, _p: &Printer, name: &str, _order: usize, _args: &[Expr]) -> Out {
        unsupported(format!("Julia: the derivative {name}' at a point is only a condition"))
    }

    fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out {
        match (name, args) {
            (RATIONAL, [a, b]) => return Ok(format!("({}//{})", p.print(a)?, p.print(b)?)),
            (BIG_RATIONAL, [e]) => return Ok(format!("Main.mr_q({})", big_rational(e)?)),
            (NUM, [e]) => return Ok(format!("Num({})", p.print(e)?)),
            (POW, [a, b]) => {
                return Ok(format!("Main.mr_pow({}, {})", p.print(a)?, p.print(b)?));
            }
            _ => {}
        }
        if name == "diff" {
            let (f, vars) = diff_args(args)?;
            // Symbolics' Differential takes its numbers, not Julia's
            let mut out = if is_constant(&f) {
                format!("Num({})", p.print(&f)?)
            } else {
                p.print(&f)?
            };
            for (v, k) in vars.iter().rev() {
                let d = if *k == 1 {
                    format!("Differential({})", var(v))
                } else {
                    format!("(Differential({})^{k})", var(v))
                };
                out = format!("{d}({out})");
            }
            return Ok(out);
        }
        if let Some((_, vars)) = self.funcs.iter().find(|f| f.0 == name) {
            if self.plain_functions {
                if args.len() != vars.len()
                    || args.iter().zip(vars).any(|(a, v)| a.as_sym() != Some(v.as_str()))
                {
                    return unsupported(format!(
                        "{name} at another point than its variables, in an equation"
                    ));
                }
                return Ok(fun(name));
            }
            return Ok(format!("{}({})", fun(name), p.args(args)?.join(", ")));
        }
        let a = p.args(args)?;
        let n = a.len();
        let f = |s: &str| Ok(format!("{s}({})", a.join(", ")));
        match (name, n) {
            (
                "exp" | "log" | "sqrt" | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "asin"
                | "acos" | "atan" | "acot" | "asec" | "acsc" | "sinh" | "cosh" | "tanh" | "coth"
                | "sech" | "csch" | "asinh" | "acosh" | "atanh" | "acoth" | "asech" | "acsch"
                | "abs" | "sign" | "floor" | "gamma" | "digamma" | "erf" | "erfc" | "erfi",
                1,
            ) => f(name),
            ("log", 2) => f("log"),
            ("atan2", 2) => f("atan"),
            ("ceiling", 1) => f("ceil"),
            ("re", 1) => f("real"),
            ("im", 1) => f("imag"),
            ("arg", 1) => f("angle"),
            ("conjugate", 1) => f("conj"),
            ("binomial" | "gamma" | "polygamma" | "beta" | "expint", 2) => f(name),
            ("log_gamma", 1) => f("loggamma"),
            ("ei", 1) => f("expinti"),
            ("si", 1) => f("sinint"),
            ("ci", 1) => f("cosint"),
            ("bessel_j", 2) => f("besselj"),
            ("bessel_y", 2) => f("bessely"),
            ("bessel_i", 2) => f("besseli"),
            ("bessel_k", 2) => f("besselk"),
            ("hankel_1", 2) => f("hankelh1"),
            ("hankel_2", 2) => f("hankelh2"),
            ("airy_ai", 1) => f("airyai"),
            ("airy_bi", 1) => f("airybi"),
            ("airy_ai_prime", 1) => f("airyaiprime"),
            ("airy_bi_prime", 1) => f("airybiprime"),
            // SpecialFunctions' zeta, ellipk and ellipe take no symbols
            ("zeta" | "elliptic_k" | "elliptic_e", _) => {
                unsupported(format!("Symbolics has no symbolic {name}"))
            }
            _ if crate::functions::NEUTRAL.contains(&name) || name == "root_of" => {
                unsupported(format!("Julia has no {name} of {n} arguments"))
            }
            _ => unsupported(format!(
                "{name} is not a function of the formats, nor an unknown function of the problem"
            )),
        }
    }
}

/// Rewrites the expression for Julia, where the arithmetic of numbers is
/// not exact: `1/2` is the float 0.5 in Julia, `sqrt(2)` the float 1.414...
/// and `2^(1/3)` the float 1.2599... A rational number is Julia's rational,
/// `1//2` (computed in big integers, as Julia's integers wrap around); a
/// function of numbers alone, as `sqrt(2)`, is called with Symbolics'
/// numbers, `sqrt(Num(2))`, which it keeps unevaluated; a power of numbers by
/// another than a whole number is Symbolics' term of it, `Main.mr_pow(2,
/// 1//3)`.
fn exact(e: &Expr) -> Expr {
    use Expr::*;
    let b = |x: &Expr| Box::new(exact(x));
    // the functions Julia lacks, written as the same function of others
    let bx = |x: Expr| Box::new(x);
    match e {
        // the principal root
        Call(name, a) if name == "root" && a.len() == 2 => {
            return exact(&Bin(
                Op::Pow,
                bx(a[0].clone()),
                bx(Bin(Op::Div, bx(Int("1".into())), bx(a[1].clone()))),
            ));
        }
        // Julia's factorial is of integers alone
        Call(name, a) if name == "factorial" && a.len() == 1 => {
            return exact(&Call(
                "gamma".into(),
                vec![Bin(Op::Add, bx(a[0].clone()), bx(Int("1".into())))],
            ));
        }
        Call(name, a) if name == "pochhammer" && a.len() == 2 => {
            let g = |x: Expr| Call("gamma".into(), vec![x]);
            return exact(&Bin(
                Op::Div,
                bx(g(Bin(Op::Add, bx(a[0].clone()), bx(a[1].clone())))),
                bx(g(a[0].clone())),
            ));
        }
        // the formats' own definition
        Call(name, a) if name == "harmonic" && a.len() == 1 => {
            return exact(&Bin(
                Op::Add,
                bx(Call(
                    "digamma".into(),
                    vec![Bin(Op::Add, bx(a[0].clone()), bx(Int("1".into())))],
                )),
                bx(Sym("euler_gamma".into())),
            ));
        }
        _ => {}
    }
    if is_rational(e) {
        return match e {
            Int(_) => e.clone(),
            Neg(x) => Neg(b(x)),
            Bin(Op::Div, x, y) if matches!(**x, Int(_)) && matches!(**y, Int(_)) => {
                Call(RATIONAL.into(), vec![(**x).clone(), (**y).clone()])
            }
            // -1/2, which the parser reads as (-1)/2
            Bin(Op::Div, x, y) if matches!(&**x, Neg(n) if matches!(**n, Int(_)))
                && matches!(**y, Int(_)) =>
            {
                let Neg(n) = &**x else { unreachable!() };
                Neg(Box::new(Call(RATIONAL.into(), vec![(**n).clone(), (**y).clone()])))
            }
            e => Call(BIG_RATIONAL.into(), vec![e.clone()]),
        };
    }
    match e {
        Bin(Op::Pow, x, y) if is_constant(x) && is_constant(y) => {
            Call(POW.into(), vec![exact(x), exact(y)])
        }
        Call(name, args)
            if crate::functions::NEUTRAL.contains(&name.as_str())
                && name != "diff"
                && args.iter().all(is_constant) =>
        {
            let num = |a: &Expr| match a {
                List(items) => List(
                    items
                        .iter()
                        .map(|i| Call(NUM.into(), vec![exact(i)]))
                        .collect(),
                ),
                a => Call(NUM.into(), vec![exact(a)]),
            };
            Call(name.clone(), args.iter().map(num).collect())
        }
        // A division by a whole number is the product with its exact rational, as Symbolics
        // keeps it: `x/3` is `x*(1//3)`, `1/(3*x)` is `(1//3)/x`. Written as a quotient,
        // SymbolicIntegration's `integrate(x/3, x)` fails on the integer power `3^-1` that
        // Symbolics makes of it, and `integrate((1//3)*x, x)` does not.
        Bin(Op::Div, x, y) if matches!(&**y, Int(n) if n != "0") && !is_constant(x) => {
            let Int(n) = &**y else { unreachable!() };
            Bin(Op::Mul, b(x), bx(Call(RATIONAL.into(), vec![Int("1".into()), Int(n.clone())])))
        }
        Bin(Op::Div, x, y) if whole_factor(y).is_some() => {
            let (c, rest) = whole_factor(y).unwrap();
            Bin(
                Op::Div,
                bx(Bin(Op::Mul, b(x), bx(Call(RATIONAL.into(), vec![Int("1".into()), c])))),
                b(&rest),
            )
        }
        Call(name, args) => Call(name.clone(), args.iter().map(exact).collect()),
        List(a) => List(a.iter().map(exact).collect()),
        Prime(n, k, a) => Prime(n.clone(), *k, a.iter().map(exact).collect()),
        Neg(x) => Neg(b(x)),
        Not(x) => Not(b(x)),
        Bin(op, x, y) => Bin(*op, b(x), b(y)),
        Rel(r, x, y) => Rel(*r, b(x), b(y)),
        And(x, y) => And(b(x), b(y)),
        Or(x, y) => Or(b(x), b(y)),
        e => e.clone(),
    }
}

/// A product with a whole number (not 0) among its factors, at any depth, as `3*x` or
/// `2*(x + 1)*sqrt(x)`, and no other number: the number and the product of the rest.
fn whole_factor(e: &Expr) -> Option<(Expr, Expr)> {
    use Expr::*;
    fn flatten<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
        match e {
            Bin(Op::Mul, a, b) => {
                flatten(a, out);
                flatten(b, out);
            }
            e => out.push(e),
        }
    }
    // a quotient of numbers alone stays as it is: `Num(pi)/6`
    if !matches!(e, Bin(Op::Mul, ..)) || is_constant(e) {
        return None;
    }
    let mut factors = Vec::new();
    flatten(e, &mut factors);
    let at = factors
        .iter()
        .position(|f| matches!(f, Int(n) if n != "0"))?;
    let c = factors[at].clone();
    let mut rest = factors
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != at)
        .map(|(_, f)| (*f).clone());
    let first = rest.next()?;
    Some((c, rest.fold(first, |acc, f| Bin(Op::Mul, Box::new(acc), Box::new(f)))))
}

/// The marks of `exact`, which the dialect prints.
const RATIONAL: &str = "\u{1}rational";
const BIG_RATIONAL: &str = "\u{1}big_rational";
const NUM: &str = "\u{1}num";
const POW: &str = "\u{1}pow";

/// A rational number in Julia's big integers, every operation in
/// parentheses.
fn big_rational(e: &Expr) -> Out {
    Ok(match e {
        Expr::Int(n) => format!("big({n})"),
        Expr::Neg(a) => format!("(-{})", big_rational(a)?),
        Expr::Bin(Op::Pow, a, k) => match k.as_int() {
            // a rational base, as a big integer has no negative powers
            Some(k) => format!("(({} // 1) ^ {k})", big_rational(a)?),
            None => return unsupported("not a rational number"),
        },
        Expr::Bin(op, a, b) => {
            let (a, b) = (big_rational(a)?, big_rational(b)?);
            match op {
                Op::Add => format!("({a} + {b})"),
                Op::Sub => format!("({a} - {b})"),
                Op::Mul => format!("({a} * {b})"),
                Op::Div => format!("({a} // {b})"),
                Op::Pow => unreachable!(),
            }
        }
        _ => return unsupported("not a rational number"),
    })
}

/// The code of one problem: the setup (symbols and functions), the task,
/// which sets `result`, and the answer, an expression of it.
struct Code {
    setup: String,
    task: String,
    answer: String,
}

fn build(p: &Problem) -> Result<Code, Unsupported> {
    let funcs = p.unknown_functions()?;
    let d = D {
        funcs: funcs.clone(),
        plain_functions: false,
    };
    let pr = Printer::new(&d);
    let pe = |e: &Expr| pr.print(&exact(e));
    let arg = |k: &str| -> Out { pe(&p.expr(k)?) };
    let list = |k: &str| -> Out {
        Ok(format!(
            "[{}]",
            p.exprs(k)?
                .iter()
                .map(&pe)
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ))
    };
    // Symbolics has no assumptions: the problem is given without them, its
    // symbols Symbolics' own (real)
    let mut setup = String::new();
    for s in p.all_symbols()? {
        setup.push_str(&format!("{} = Symbolics.variable(:{s})\n", var(&s)));
    }
    let mut answer = "result".to_string();
    let task = match p.op.as_str() {
        "integrate" => {
            if p.has("from") || p.has("to") {
                return unsupported(
                    "Symbolics has no definite integral: SymbolicIntegration gives \
                     antiderivatives",
                );
            }
            format!(
                "result = SymbolicIntegration.integrate({}, {})",
                arg("integrand")?,
                var(&p.symbol("var")?)
            )
        }
        "solve_ode" => return ode(p, &funcs, setup),
        "solve_pde" => return unsupported("Symbolics has no PDE solver"),
        "sum" | "product" => return unsupported(format!("Symbolics has no symbolic {}", p.op)),
        "solve_recurrence" => return unsupported("Symbolics has no recurrence solver"),
        "limit" => {
            let point = p.expr("point")?;
            let side = match p.direction()? {
                Some('+') => ":right",
                Some(_) => ":left",
                None => ":both",
            };
            let infinite = matches!(&point, Expr::Sym(s) if s == "infinity")
                || matches!(&point, Expr::Neg(a) if a.as_sym() == Some("infinity"));
            // at an infinity the one side is Symbolics' default
            let side = if infinite { String::new() } else { format!(", {side}") };
            // limit takes Symbolics' expressions unwrapped, not as Num
            format!(
                "result = Symbolics.limit(Symbolics.unwrap({}), Symbolics.unwrap({}), {}{side})",
                arg("expr")?,
                var(&p.symbol("var")?),
                pe(&point)?
            )
        }
        "series" => {
            let order = p.int("order")?;
            let point = p.expr("point")?;
            if point.has_any(&["infinity".to_string()]) || matches!(&point, Expr::Sym(s) if s == "infinity") {
                return unsupported("Symbolics' taylor expands at a point, not at infinity");
            }
            if order <= 0 {
                return unsupported(
                    "Symbolics' taylor has no negative powers, and the series has no others",
                );
            }
            format!(
                "result = Symbolics.taylor({}, {}, {}, 0:{})",
                arg("expr")?,
                var(&p.symbol("var")?),
                pe(&point)?,
                order - 1
            )
        }
        "laplace" | "inverse_laplace" => {
            p.check_convention()?;
            let (f, x, y) = (
                arg("expr")?,
                p.symbol("var")?,
                p.symbol("transform_var")?,
            );
            // the transform's function names, which Symbolics' laplace takes
            // for its rules of an unknown f(t)
            if p.op == "laplace" {
                setup.push_str(&format!("@syms mr_f({x})::Real mr_F({y})::Real\n"));
                format!(
                    "result = Symbolics.laplace({f}, mr_f, {}, mr_F, {})",
                    var(&x),
                    var(&y)
                )
            } else {
                setup.push_str(&format!("@syms mr_F({x})::Real mr_f({y})::Real\n"));
                format!(
                    "result = Symbolics.inverse_laplace({f}, mr_F, {}, mr_f, {})",
                    var(&x),
                    var(&y)
                )
            }
        }
        "fourier" | "inverse_fourier" | "mellin" | "inverse_mellin" | "z_transform"
        | "inverse_z_transform" => {
            return unsupported(format!("Symbolics has no {}", p.op.replace('_', " ")));
        }
        "solve" => {
            let eqs = p.exprs("equations")?;
            if eqs
                .iter()
                .any(|e| e.is_relation() && !matches!(e, Expr::Rel(Rel::Eq, ..)))
            {
                return unsupported("Symbolics' symbolic_solve takes no inequalities");
            }
            let vars: Vec<String> = p.symbols("vars")?.iter().map(|v| var(v)).collect();
            // an expression alone is an equation = 0, as symbolic_solve takes it
            let printed = eqs.iter().map(&pe).collect::<Result<Vec<_>, _>>()?;
            answer = format!("Main.mr_solutions(result, [{}])", vars.join(", "));
            if printed.len() == 1 && vars.len() == 1 {
                format!(
                    "result = Symbolics.symbolic_solve({}, {})",
                    printed[0], vars[0]
                )
            } else {
                format!(
                    "result = Symbolics.symbolic_solve([{}], [{}])",
                    printed.join(", "),
                    vars.join(", ")
                )
            }
        }
        "holds" => return unsupported("Symbolics has no decision of a relation"),
        "simplify" => format!("result = Symbolics.simplify({})", arg("expr")?),
        "evaluate" => format!(
            "result = Symbolics.simplify(Symbolics.expand_derivatives({}))",
            arg("expr")?
        ),
        "hyperexpand" => return unsupported("Symbolics has no hypergeometric functions"),
        "partial_fractions" => format!(
            "result = Symbolics.partial_frac_decomposition({}, {})",
            arg("expr")?,
            var(&p.symbol("var")?)
        ),
        "factor" | "gcd" | "lcm" | "resultant" | "discriminant" | "groebner"
        | "minimal_polynomial" => return polynomial(p, &pe, &list),
        "determinant" | "inverse" | "rank" | "nullspace" | "eigenvalues" | "eigenvectors"
        | "jordan_form" | "solve_linear" | "matrix_power" => {
            return linear_algebra(p, &pe, setup);
        }
        op => return unsupported(format!("the op {op} is not known to the runner")),
    };
    Ok(Code {
        setup,
        task,
        answer,
    })
}

/// The highest order of a derivative in the expression.
fn order(e: &Expr) -> usize {
    match e {
        Expr::Call(n, args) if n == "diff" => diff_args(args)
            .map(|(_, v)| v.iter().map(|x| x.1).sum())
            .unwrap_or(0),
        Expr::Call(_, a) | Expr::List(a) | Expr::Prime(_, _, a) => {
            a.iter().map(order).max().unwrap_or(0)
        }
        Expr::Neg(a) | Expr::Not(a) => order(a),
        Expr::Bin(_, a, b) | Expr::Rel(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
            order(a).max(order(b))
        }
        _ => 0,
    }
}

/// One ODE: without conditions `symbolic_solve_ode(eq, y, x)`, the unknown
/// function a plain variable `y`; with its initial values at 0, `y(0)`,
/// `y'(0)`, ... up to the order, `laplace_solve_ode(eq, y, x, [values])`.
/// Symbolics solves a system only as `x' = A x` with every initial value
/// (`solve_linear_ode_system`), which the runner does not rewrite the
/// equations into.
fn ode(p: &Problem, funcs: &[(String, Vec<String>)], mut setup: String) -> Result<Code, Unsupported> {
    let eqs = p.exprs("equations")?;
    if eqs.len() != 1 || funcs.len() != 1 {
        return unsupported(
            "Symbolics solves a system of ODEs only as x' = A x with its initial values",
        );
    }
    let (y, xs) = &funcs[0];
    if xs.len() != 1 {
        return unsupported("an unknown function of several variables");
    }
    let x = &xs[0];
    let conds = conditions(p, "conditions")?;
    let n = order(&eqs[0]);
    // the solution, y(x) = ..., the function written as a call
    let call = format!("only(@variables {y}(..))({})", var(x));
    let answer = format!("Main.mr_ode_solutions({call}, result)");
    if conds.is_empty() {
        let d = D {
            funcs: funcs.to_vec(),
            plain_functions: true,
        };
        let pr = Printer::new(&d);
        setup.push_str(&format!("{} = Symbolics.variable(:{y})\n", fun(y)));
        return Ok(Code {
            setup,
            task: format!(
                "result = Symbolics.symbolic_solve_ode({}, {}, {})",
                pr.print(&exact(&eqs[0]))?,
                fun(y),
                var(x)
            ),
            answer,
        });
    }
    let mut values = vec![None; n];
    for c in &conds {
        let at_zero = c.at.len() == 1 && matches!(&c.at[0], Expr::Int(z) if z == "0");
        if !at_zero || c.order >= n || values[c.order].is_some() {
            return unsupported(
                "Symbolics' laplace_solve_ode takes the initial values y(0), y'(0), ... up to \
                 the order, and no other conditions",
            );
        }
        values[c.order] = Some(c.value.clone());
    }
    let d = D {
        funcs: funcs.to_vec(),
        plain_functions: false,
    };
    let pr = Printer::new(&d);
    let mut vs = Vec::new();
    for v in values {
        match v {
            Some(v) => vs.push(pr.print(&exact(&v))?),
            None => {
                return unsupported(
                    "Symbolics' laplace_solve_ode takes every initial value y(0), y'(0), ... up \
                     to the order",
                );
            }
        }
    }
    setup.push_str(&format!("{} = only(@syms {y}({x})::Real)\n", fun(y)));
    Ok(Code {
        setup,
        task: format!(
            "result = Symbolics.laplace_solve_ode({}, {}, {}, [{}])",
            pr.print(&exact(&eqs[0]))?,
            fun(y),
            var(x),
            vs.join(", ")
        ),
        answer,
    })
}

/// A Julia string literal: `"`, `\` and `$` escaped, a line break as `\n`.
fn jl_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Adapter for Julia {
    /// Julia's own version alone, `1.12.6`: the driver prints it with the packages' versions
    /// (`1.12.6, Symbolics 7.49.0, SymbolicIntegration 3.8.1`), which are those of the runner
    /// release's `julia/Manifest.toml`, and the system's version on the site is Julia's.
    fn clean_version(&self, text: &str) -> String {
        text.split(',').next().unwrap_or(text).trim().to_string()
    }

    fn name(&self) -> &'static str {
        "Julia"
    }

    fn slug(&self) -> &'static str {
        "julia"
    }

    fn default_command(&self) -> &'static str {
        "julia"
    }

    /// The packages are those of Julia's shared environment `math-run`
    /// (`scripts/julia-setup.sh` installs it from `julia/Manifest.toml`),
    /// unless `JULIA_PROJECT` names another.
    fn spec(&self, program: &str) -> Spec {
        let mut args: Vec<String> = Vec::new();
        if std::env::var_os("JULIA_PROJECT").is_none() {
            args.push(format!("--project=@{JULIA_ENVIRONMENT}"));
        }
        args.extend([
            "--startup-file=no".into(),
            "--history-file=no".into(),
            "--color=no".into(),
            "-e".into(),
            DRIVER.into(),
        ]);
        Spec {
            program: program.to_string(),
            args,
            env: vec![],
            dir: None,
        }
    }

    /// Julia compiles each function at its first call: the driver first
    /// runs a small problem of each op, as the adapter writes them, so that
    /// the problems' times are of the work and not of compiling the
    /// commands (a problem's own functions are still compiled at their first
    /// use).
    fn start(&self, nonce: &str) -> String {
        let mut out = String::new();
        for p in warm_up_problems() {
            if let Ok(c) = build(&p) {
                out.push_str(&format!(
                    "mr_warm({}, {}, {})\n",
                    jl_string(&c.setup),
                    jl_string(&c.task),
                    jl_string(&c.answer)
                ));
            }
        }
        out.push_str(&format!("mr_start({})", jl_string(nonce)));
        out
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        let c = build(p)?;
        Ok(format!(
            "mr_problem({}, {}, {}, {})",
            jl_string(nonce),
            jl_string(&c.setup),
            jl_string(&c.task),
            jl_string(&c.answer)
        ))
    }

    fn goodbye(&self) -> &'static str {
        "mr_quit()"
    }

    /// Julia compiles what the warm-up runs, at the start: minutes, the
    /// first time on a machine.
    fn start_time(&self) -> std::time::Duration {
        std::time::Duration::from_secs(900)
    }

    fn read_answer(&self, text: &str) -> Result<String, String> {
        read_answer(text)
    }
}

/// A small problem of each op, to warm up: as (op, args).
const WARM_UP: &[(&str, &str)] = &[
    ("integrate", r#"{"integrand": "x*exp(x) + 1/(1 + x^2) + sqrt(x + 1)", "var": "x"}"#),
    ("solve_ode", r#"{"equations": ["diff(y(x), x) + y(x) = x"], "function": "y(x)"}"#),
    (
        "solve_ode",
        r#"{"equations": ["diff(y(x), x, 2) + y(x) = 0"], "function": "y(x)", "conditions": ["y(0) = 1", "subs(diff(y(x), x), x = 0) = 0"]}"#,
    ),
    ("limit", r#"{"expr": "(exp(x) - 1)/x", "var": "x", "point": "0"}"#),
    ("limit", r#"{"expr": "log(x)/x", "var": "x", "point": "infinity"}"#),
    ("series", r#"{"expr": "exp(x)*sin(x)", "var": "x", "point": "0", "order": 3}"#),
    ("laplace", r#"{"expr": "exp(-t)*sin(t)", "var": "t", "transform_var": "s"}"#),
    ("inverse_laplace", r#"{"expr": "1/(s^2 + 3*s + 2)", "var": "s", "transform_var": "t"}"#),
    ("solve", r#"{"equations": ["x^2 - 3*x + 2 = 0"], "vars": ["x"], "domain": "complex"}"#),
    ("solve", r#"{"equations": ["exp(2*x) = 3"], "vars": ["x"], "domain": "real"}"#),
    (
        "solve",
        r#"{"equations": ["x + y = 3", "x*y = 2"], "vars": ["x", "y"], "domain": "complex"}"#,
    ),
    ("simplify", r#"{"expr": "sin(x)^2 + cos(x)^2 + (x^2 - 1)/(x - 1)"}"#),
    ("evaluate", r#"{"expr": "gamma(1/2) + sqrt(2)*diff(x^3, x) + bessel_j(1, x)"}"#),
    ("partial_fractions", r#"{"expr": "(x + 3)/(x^3 - x)", "var": "x"}"#),
    ("factor", r#"{"expr": "x^4 - 1", "var": "x"}"#),
    ("gcd", r#"{"polynomials": ["x^2 - 1", "x^2 + 2*x + 1"], "var": "x"}"#),
    ("groebner", r#"{"polynomials": ["x^2 + y", "x*y - 1"], "vars": ["x", "y"], "order": "lex"}"#),
    ("determinant", r#"{"matrix": "[[1, a], [2, 3]]"}"#),
    ("inverse", r#"{"matrix": "[[1, a], [2, 3]]"}"#),
    ("rank", r#"{"matrix": "[[1, 2], [2, 4]]"}"#),
    ("nullspace", r#"{"matrix": "[[1, 2], [2, 4]]"}"#),
    ("solve_linear", r#"{"matrix": "[[1, 1], [1, -1]]", "rhs": "[3, 1]"}"#),
];

fn warm_up_problems() -> Vec<Problem> {
    WARM_UP
        .iter()
        .enumerate()
        .filter_map(|(k, (op, args))| {
            let args: serde_json::Value = serde_json::from_str(args).ok()?;
            Problem::from_json(&serde_json::json!({
                "id": format!("warm-up-{k}"),
                "category": "warm_up",
                "problem": {"op": op, "args": args},
            }))
            .ok()
        })
        .collect()
}

/// The code the adapter writes for a problem, for `math-run translate`.
pub fn show(p: &Problem) -> Result<String, Unsupported> {
    let c = build(p)?;
    Ok(format!("{}{}\nanswer = {}", c.setup, c.task, c.answer))
}

/// Whether the expression is a polynomial in its symbols with rational
/// coefficients, as Nemo's polynomial rings take it.
fn is_rational_polynomial(e: &Expr) -> bool {
    match e {
        Expr::Int(_) => true,
        Expr::Sym(s) => !crate::expr::is_constant(s),
        Expr::Neg(a) => is_rational_polynomial(a),
        Expr::Bin(Op::Add | Op::Sub | Op::Mul, a, b) => {
            is_rational_polynomial(a) && is_rational_polynomial(b)
        }
        Expr::Bin(Op::Div, a, b) => is_rational_polynomial(a) && is_rational(b),
        Expr::Bin(Op::Pow, a, b) => {
            is_rational_polynomial(a) && b.as_int().is_some_and(|k| k >= 0)
        }
        _ => false,
    }
}

/// The polynomial ops. Symbolics has no factor, gcd, resultant or
/// discriminant of its expressions: these are Nemo's, in its polynomial
/// rings over the rationals (`polynomial_ring(QQ, [...])`), the resultant
/// and the discriminant in `R[x]` with R the ring of the other symbols. The
/// Gröbner basis is Symbolics' `groebner_basis` (Groebner.jl); the minimal
/// polynomial Nemo's `minpoly` of the algebraic number in `QQBar`.
fn polynomial(
    p: &Problem,
    pe: &dyn Fn(&Expr) -> Out,
    list: &dyn Fn(&str) -> Out,
) -> Result<Code, Unsupported> {
    let ring = |names: &[String]| -> String {
        format!(
            "mr_R, ({},) = Nemo.polynomial_ring(Nemo.QQ, [{}])\n",
            names.iter().map(|v| var(v)).collect::<Vec<_>>().join(", "),
            names.iter().map(|v| format!(":{v}")).collect::<Vec<_>>().join(", ")
        )
    };
    let polys = |key: &str| -> Result<Vec<Expr>, Unsupported> {
        let ps = if key == "expr" { vec![p.expr(key)?] } else { p.exprs(key)? };
        if ps.iter().any(|e| !is_rational_polynomial(e)) {
            return unsupported(
                "not a polynomial with rational coefficients, as Nemo's polynomial ring takes it",
            );
        }
        Ok(ps)
    };
    let symbols = p.all_symbols()?;
    let code = |setup: String, task: String| {
        Ok(Code {
            setup,
            task,
            answer: "result".into(),
        })
    };
    match p.op.as_str() {
        "factor" => {
            if poly_domain(p)? != PolyDomain::Rationals {
                return unsupported("Nemo's factor is over the rationals here, not the Gaussian rationals");
            }
            let e = &polys("expr")?[0];
            if symbols.is_empty() {
                return unsupported("a number, not a polynomial");
            }
            code(ring(&symbols), format!("result = Nemo.factor({})", pe(e)?))
        }
        "gcd" | "lcm" => {
            let ps = polys("polynomials")?;
            if symbols.is_empty() || ps.is_empty() {
                return unsupported("no polynomials");
            }
            code(
                ring(&symbols),
                format!("result = reduce(Nemo.{}, {})", p.op, list("polynomials")?),
            )
        }
        "resultant" | "discriminant" => {
            let x = p.symbol("var")?;
            let ps = if p.op == "resultant" { polys("polynomials")? } else { polys("expr")? };
            if p.op == "resultant" && ps.len() != 2 {
                return unsupported("a resultant of other than two polynomials");
            }
            let others: Vec<String> = symbols.into_iter().filter(|s| *s != x).collect();
            let (mut setup, base) = if others.is_empty() {
                (String::new(), "Nemo.QQ")
            } else {
                (ring(&others), "mr_R")
            };
            setup.push_str(&format!(
                "mr_S, {} = Nemo.polynomial_ring({base}, :{x})\n",
                var(&x)
            ));
            let args = ps.iter().map(pe).collect::<Result<Vec<_>, _>>()?;
            code(
                setup,
                format!("result = Nemo.{}({})", p.op, args.join(", ")),
            )
        }
        "groebner" => {
            let order = match monomial_order(p)? {
                MonomialOrder::Lex => "Lex",
                MonomialOrder::Grlex => "DegLex",
                MonomialOrder::Grevlex => "DegRevLex",
            };
            let vars: Vec<String> = p.symbols("vars")?.iter().map(|v| var(v)).collect();
            let mut setup = String::new();
            for s in &symbols {
                setup.push_str(&format!("{} = Symbolics.variable(:{s})\n", var(s)));
            }
            code(
                setup,
                format!(
                    "result = Symbolics.groebner_basis({}, ordering = Groebner.{order}({}))",
                    list("polynomials")?,
                    vars.join(", ")
                ),
            )
        }
        "minimal_polynomial" => {
            let x = p.symbol("var")?;
            let a = qqbar(&p.expr("expr")?)?;
            code(
                format!(
                    "mr_K = Nemo.algebraic_closure(Nemo.QQ)\nmr_R, _ = Nemo.polynomial_ring(Nemo.QQ, :{x})\n"
                ),
                format!("result = Nemo.minpoly(mr_R, {a})"),
            )
        }
        op => unsupported(format!("the op {op} is not known to the runner")),
    }
}

/// An algebraic number as Nemo's `QQBar` builds it, exactly: rationals,
/// `+ - * /`, whole and rational powers, roots, and the cosine and sine of
/// rational multiples of pi. Another number (pi, e, gamma(1/4), 2^sqrt(2))
/// is not one QQBar has.
fn qqbar(e: &Expr) -> Out {
    let no = || unsupported("not an algebraic number that Nemo's QQBar builds (radicals, cos and sin of rational multiples of pi)");
    // a rational multiple of pi: the rational
    fn of_pi(e: &Expr) -> Option<Expr> {
        let pi = |x: &Expr| matches!(x, Expr::Sym(s) if s == "pi");
        let one = || Expr::Int("1".into());
        match e {
            x if pi(x) => Some(one()),
            Expr::Neg(a) => of_pi(a).map(|r| Expr::Neg(Box::new(r))),
            Expr::Bin(Op::Mul, a, b) if pi(b) && is_rational(a) => Some((**a).clone()),
            Expr::Bin(Op::Mul, a, b) if pi(a) && is_rational(b) => Some((**b).clone()),
            Expr::Bin(Op::Div, a, b) if is_rational(b) => {
                of_pi(a).map(|r| Expr::Bin(Op::Div, Box::new(r), b.clone()))
            }
            _ => None,
        }
    }
    Ok(match e {
        e if is_rational(e) => format!("mr_K({})", big_rational(e)?),
        Expr::Neg(a) => format!("(-{})", qqbar(a)?),
        Expr::Bin(op, a, b) if *op != Op::Pow => {
            let o = match op {
                Op::Add => "+",
                Op::Sub => "-",
                Op::Mul => "*",
                _ => "/",
            };
            format!("({} {o} {})", qqbar(a)?, qqbar(b)?)
        }
        Expr::Bin(Op::Pow, a, b) => match (b.as_int(), b.as_ref()) {
            (Some(k), _) => format!("({} ^ {k})", qqbar(a)?),
            (None, Expr::Bin(Op::Div, n, d)) => match (n.as_int(), d.as_int()) {
                (Some(n), Some(d)) if d > 0 => {
                    format!("(Nemo.root({}, {d}) ^ {n})", qqbar(a)?)
                }
                _ => return no(),
            },
            (None, Expr::Neg(r)) => match r.as_ref() {
                Expr::Bin(Op::Div, n, d) => match (n.as_int(), d.as_int()) {
                    (Some(n), Some(d)) if d > 0 => {
                        format!("(Nemo.root({}, {d}) ^ (-{n}))", qqbar(a)?)
                    }
                    _ => return no(),
                },
                _ => return no(),
            },
            _ => return no(),
        },
        Expr::Call(f, a) if f == "sqrt" && a.len() == 1 => format!("sqrt({})", qqbar(&a[0])?),
        Expr::Call(f, a) if f == "root" && a.len() == 2 => match a[1].as_int() {
            Some(k) if k > 0 => format!("Nemo.root({}, {k})", qqbar(&a[0])?),
            _ => return no(),
        },
        Expr::Call(f, a) if (f == "cos" || f == "sin") && a.len() == 1 => match of_pi(&a[0]) {
            Some(r) => format!("Nemo.{f}pi(mr_K({}))", big_rational(&r)?),
            None => return no(),
        },
        _ => return no(),
    })
}

/// The linear algebra ops: `det` and `inv` of Julia's LinearAlgebra on a
/// matrix of Symbolics' numbers (exact, by Symbolics' arithmetic);
/// Symbolics' `symbolic_linear_solve` of a square system; the rank and the
/// null space Nemo's, of a matrix of rationals. Symbolics has no
/// eigenvalues, Jordan form or power of a symbolic exponent.
fn linear_algebra(p: &Problem, pe: &dyn Fn(&Expr) -> Out, setup: String) -> Result<Code, Unsupported> {
    let m = p.expr("matrix")?;
    let rows = match &m {
        Expr::List(rows) if !rows.is_empty() => rows.clone(),
        _ => return unsupported("the matrix is not a list of rows"),
    };
    let cells: Vec<Expr> = rows.iter().flat_map(|r| r.items()).collect();
    let n = rows[0].items().len();
    if rows.iter().any(|r| !matches!(r, Expr::List(c) if c.len() == n)) {
        return unsupported("the matrix's rows are not lists of the same length");
    }
    let mat = format!("Main.mr_matrix({})", pe(&m)?);
    let rational = || -> Result<String, Unsupported> {
        if !cells.iter().all(is_rational) {
            return unsupported(format!(
                "Symbolics has no {}: Nemo's is of a matrix of rationals",
                p.op
            ));
        }
        Ok(format!("Main.mr_qmatrix({})", pe(&m)?))
    };
    let mut answer = "result".to_string();
    let task = match p.op.as_str() {
        "determinant" if p.has("power") => {
            return unsupported("Symbolics has no power of a matrix by a symbolic exponent");
        }
        "determinant" => format!("result = LinearAlgebra.det({mat})"),
        "inverse" => format!("result = LinearAlgebra.inv({mat})"),
        "rank" => format!("result = Nemo.rank({})", rational()?),
        "nullspace" => {
            answer = "Main.mr_columns(result[2])".into();
            format!("result = Nemo.nullspace({})", rational()?)
        }
        "solve_linear" => {
            if rows.len() != n {
                return unsupported("Symbolics' symbolic_linear_solve takes as many equations as unknowns");
            }
            format!(
                "mr_v = [Symbolics.variable(Symbol(\"mr_v\", j)) for j in 1:{n}]\n\
                 result = Symbolics.symbolic_linear_solve({mat} * mr_v .~ {}, mr_v)",
                pe(&p.expr("rhs")?)?
            )
        }
        op => return unsupported(format!("Symbolics has no {}", op.replace('_', " "))),
    };
    Ok(Code { setup, task, answer })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(op: &str, args: &str) -> Problem {
        Problem::from_json(&serde_json::json!({
            "id": "t",
            "category": "test",
            "problem": {"op": op, "args": serde_json::from_str::<serde_json::Value>(args).unwrap()},
        }))
        .unwrap()
    }

    fn code(op: &str, args: &str) -> Code {
        build(&problem(op, args)).unwrap()
    }

    fn why(op: &str, args: &str) -> String {
        match build(&problem(op, args)) {
            Err(u) => u.0,
            Ok(c) => panic!("supported: {}", c.task),
        }
    }

    /// An expression of the problem, as the adapter gives it to Julia.
    fn jl(e: &str) -> String {
        let p = problem("simplify", &serde_json::json!({ "expr": e }).to_string());
        let c = build(&p).unwrap();
        c.task
            .strip_prefix("result = Symbolics.simplify(")
            .and_then(|t| t.strip_suffix(')'))
            .unwrap()
            .to_string()
    }

    #[test]
    fn the_version_is_julias_alone() {
        assert_eq!(
            Julia.clean_version("1.12.6, Symbolics 7.49.0, SymbolicIntegration 3.8.1\n"),
            "1.12.6"
        );
        assert_eq!(Julia.clean_version("1.12.6"), "1.12.6");
    }

    #[test]
    fn a_division_by_a_whole_number_is_the_product_with_its_rational() {
        // SymbolicIntegration's integrate(x/3, x) and integrate(1/(3*x), x) fail on the integer
        // power 3^-1 that Symbolics makes of the quotient; the rational form works
        assert_eq!(jl("x/3"), "s_x*(1//3)");
        assert_eq!(jl("1/(3*x)"), "1*(1//3)/s_x");
        assert_eq!(jl("3/(4*x)"), "3*(1//4)/s_x");
        assert_eq!(jl("x/(2*(x + 1)*sqrt(x))"), "s_x*(1//2)/((s_x + 1)*sqrt(s_x))");
        assert_eq!(jl("(x + 1)/5"), "(s_x + 1)*(1//5)");
        // nothing changes where there is no whole number to divide by, or it is 0
        assert_eq!(jl("1/x"), "1/s_x");
        assert_eq!(jl("x/y"), "s_x/s_y");
        assert_eq!(jl("1/(x*y)"), "1/(s_x*s_y)");
    }

    #[test]
    fn numbers_stay_exact() {
        assert_eq!(jl("x^(1/2) + 1/3"), "s_x^(1//2) + (1//3)");
        assert_eq!(jl("-1/2*x"), "-(1//2)*s_x");
        assert_eq!(jl("x/2 + 2/x"), "s_x*(1//2) + 2/s_x");
        // Julia's sqrt(2) is a float, sqrt(Num(2)) Symbolics' sqrt
        assert_eq!(jl("sqrt(2)*x"), "sqrt(Num(2))*s_x");
        assert_eq!(jl("gamma(1/2)"), "gamma(Num((1//2)))");
        assert_eq!(jl("2^(1/3)"), "Main.mr_pow(2, (1//3))");
        assert_eq!(jl("root(2, 3) + root(x, 3)"), "Main.mr_pow(2, (1//3)) + s_x^(1//3)");
        assert_eq!(jl("sin(pi/6)*x"), "sin(Num(Num(π)/6))*s_x");
        // Julia's integers wrap around: big integers
        assert_eq!(jl("2^70*x"), "Main.mr_q(((big(2) // 1) ^ 70))*s_x");
        assert_eq!(jl("(1/2 + 1/3)*x"), "Main.mr_q(((big(1) // big(2)) + (big(1) // big(3))))*s_x");
        assert_eq!(jl("123456789012345678901234567890*x"), "123456789012345678901234567890*s_x");
        assert_eq!(jl("pi*x + e^x + i"), "Num(π)*s_x + Num(ℯ)^s_x + im");
        assert_eq!(jl("euler_gamma"), "Num(Base.MathConstants.eulergamma)");
        assert_eq!(jl("diff(1/2, x) + diff(x^2, x)"), "Differential(s_x)(Num((1//2))) + Differential(s_x)(s_x^2)");
    }

    #[test]
    fn functions_by_their_julia_names() {
        assert_eq!(jl("bessel_j(1/2, x)"), "besselj((1//2), s_x)");
        assert_eq!(jl("hankel_1(0, x)"), "hankelh1(0, s_x)");
        assert_eq!(jl("log(2, x) + atan2(y, x)"), "log(2, s_x) + atan(s_y, s_x)");
        assert_eq!(jl("log_gamma(x) + ei(x)"), "loggamma(s_x) + expinti(s_x)");
        assert_eq!(jl("ceiling(x) + re(x) + conjugate(x)"), "ceil(s_x) + real(s_x) + conj(s_x)");
        assert_eq!(jl("factorial(x)"), "gamma(s_x + 1)");
        assert_eq!(jl("pochhammer(x, 3)"), "gamma(s_x + 3)/gamma(s_x)");
        assert_eq!(
            jl("harmonic(x)"),
            "digamma(s_x + 1) + Num(Base.MathConstants.eulergamma)"
        );
        for f in [
            "lambert_w(x)",
            "polylog(2, x)",
            "hypergeometric([1], [2], x)",
            "legendre_p(2, x)",
            "elliptic_f(x, 1/2)",
            "elliptic_k(x)",
            "zeta(x)",
            "heaviside(x)",
            "fresnel_s(x)",
            "li(x)",
        ] {
            let p = problem("simplify", &serde_json::json!({ "expr": f }).to_string());
            assert!(build(&p).is_err(), "{f}");
        }
    }

    #[test]
    fn the_ops() {
        let c = code("integrate", r#"{"integrand": "2*x*exp(x^2)", "var": "x"}"#);
        assert_eq!(c.setup, "s_x = Symbolics.variable(:x)\n");
        assert_eq!(
            c.task,
            "result = SymbolicIntegration.integrate(2*s_x*exp(s_x^2), s_x)"
        );
        assert!(why("integrate", r#"{"integrand": "x", "var": "x", "from": "0", "to": "1"}"#)
            .contains("definite"));
        let c = code("limit", r#"{"expr": "1/x", "var": "x", "point": "0", "direction": "+"}"#);
        assert_eq!(
            c.task,
            "result = Symbolics.limit(Symbolics.unwrap(1/s_x), Symbolics.unwrap(s_x), 0, :right)"
        );
        let c = code("limit", r#"{"expr": "1/x", "var": "x", "point": "-infinity"}"#);
        assert_eq!(
            c.task,
            "result = Symbolics.limit(Symbolics.unwrap(1/s_x), Symbolics.unwrap(s_x), -Inf)"
        );
        let c = code("series", r#"{"expr": "exp(x)", "var": "x", "point": "1", "order": 4}"#);
        assert_eq!(c.task, "result = Symbolics.taylor(exp(s_x), s_x, 1, 0:3)");
        assert!(why("series", r#"{"expr": "exp(1/x)", "var": "x", "point": "infinity", "order": 3}"#)
            .contains("infinity"));
        let c = code("solve", r#"{"equations": ["x^2 = 4"], "vars": ["x"], "domain": "real"}"#);
        assert_eq!(c.task, "result = Symbolics.symbolic_solve(s_x^2 ~ 4, s_x)");
        assert_eq!(c.answer, "Main.mr_solutions(result, [s_x])");
        let c = code(
            "solve",
            r#"{"equations": ["x + y = 3", "x - y = 1"], "vars": ["x", "y"]}"#,
        );
        assert_eq!(
            c.task,
            "result = Symbolics.symbolic_solve([s_x + s_y ~ 3, s_x - s_y ~ 1], [s_x, s_y])"
        );
        assert!(why("solve", r#"{"equations": ["x^2 > 1"], "vars": ["x"], "domain": "real"}"#)
            .contains("inequalities"));
        let c = code(
            "inverse_laplace",
            r#"{"expr": "1/(s^2 + 1)", "var": "s", "transform_var": "t"}"#,
        );
        assert_eq!(
            c.task,
            "result = Symbolics.inverse_laplace(1/(s_s^2 + 1), mr_F, s_s, mr_f, s_t)"
        );
        for op in ["sum", "product", "solve_recurrence", "solve_pde", "fourier", "holds"] {
            assert!(build(&problem(op, "{}")).is_err(), "{op}");
        }
    }

    #[test]
    fn odes() {
        let c = code(
            "solve_ode",
            r#"{"equations": ["diff(y(x), x) = 2*y(x)"], "function": "y(x)"}"#,
        );
        assert_eq!(
            c.task,
            "result = Symbolics.symbolic_solve_ode(Differential(s_x)(f_y) ~ 2*f_y, f_y, s_x)"
        );
        assert!(c.setup.contains("f_y = Symbolics.variable(:y)\n"));
        assert_eq!(
            c.answer,
            "Main.mr_ode_solutions(only(@variables y(..))(s_x), result)"
        );
        let c = code(
            "solve_ode",
            r#"{"equations": ["diff(y(x), x, 2) + y(x) = 0"], "function": "y(x)",
                "conditions": ["y(0) = 1", "subs(diff(y(x), x), x = 0) = 1/2"]}"#,
        );
        assert_eq!(
            c.task,
            "result = Symbolics.laplace_solve_ode((Differential(s_x)^2)(f_y(s_x)) + f_y(s_x) ~ 0, \
             f_y, s_x, [1, (1//2)])"
        );
        // conditions elsewhere than 0, or not every one
        assert!(build(&problem(
            "solve_ode",
            r#"{"equations": ["diff(y(x), x, 2) + y(x) = 0"], "function": "y(x)",
                "conditions": ["y(1) = 1", "subs(diff(y(x), x), x = 1) = 0"]}"#,
        ))
        .is_err());
        assert!(build(&problem(
            "solve_ode",
            r#"{"equations": ["diff(y(x), x, 2) + y(x) = 0"], "function": "y(x)",
                "conditions": ["y(0) = 1"]}"#,
        ))
        .is_err());
        assert!(build(&problem(
            "solve_ode",
            r#"{"equations": ["diff(f(t), t) = g(t)", "diff(g(t), t) = -f(t)"],
                "functions": ["f(t)", "g(t)"]}"#,
        ))
        .is_err());
    }

    #[test]
    fn requests_are_julia_strings() {
        assert_eq!(jl_string("a\"b\\c$d\ne"), r#""a\"b\\c\$d\ne""#);
        let r = Julia
            .problem(
                &problem("simplify", r#"{"expr": "x"}"#),
                "n-1",
            )
            .unwrap();
        assert_eq!(
            r,
            r#"mr_problem("n-1", "s_x = Symbolics.variable(:x)\n", "result = Symbolics.simplify(s_x)", "result")"#
        );
        assert!(!r.contains('\n'));
    }
}
