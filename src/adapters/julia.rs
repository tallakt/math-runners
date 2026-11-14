//! Julia, with Symbolics.jl, driven by a small Julia program
//! (`julia_driver.jl`) kept running as a session, which runs the code this
//! adapter writes for each problem in a fresh module. The answer is read
//! from Symbolics' expression tree into the neutral syntax
//! (`julia_read.rs`).

use super::julia_read::read_answer;
use super::{Adapter, conditions};
use crate::expr::{Expr, Op, Rel};
use crate::print::{Dialect, Out, Printer, Unsupported, diff_args, unsupported};
use crate::problem::Problem;
use crate::session::Spec;

pub struct Julia;

const DRIVER: &str = include_str!("julia_driver.jl");

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
        Expr::Call(n, a) => !crate::functions::NEUTRAL.contains(&n.as_str()) || a.iter().any(has_unknown_call),
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
            let mut out = p.print(&f)?;
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
                | "abs" | "sign" | "floor" | "factorial" | "gamma" | "digamma" | "erf" | "erfc"
                | "erfi" | "zeta",
                1,
            ) => f(name),
            ("log", 2) => f("log"),
            ("atan2", 2) => f("atan"),
            ("ceiling", 1) => f("ceil"),
            ("re", 1) => f("real"),
            ("im", 1) => f("imag"),
            ("arg", 1) => f("angle"),
            ("conjugate", 1) => f("conj"),
            ("binomial" | "gamma" | "polygamma" | "beta" | "expint" | "zeta", 2) => f(name),
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
            ("elliptic_k", 1) => f("ellipk"),
            ("elliptic_e", 1) => f("ellipe"),
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
            format!(
                "result = Symbolics.limit({}, {}, {}{side})",
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
    fn name(&self) -> &'static str {
        "Julia"
    }

    fn slug(&self) -> &'static str {
        "julia"
    }

    fn default_command(&self) -> &'static str {
        "julia"
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: program.to_string(),
            args: vec![
                "--startup-file=no".into(),
                "--history-file=no".into(),
                "--color=no".into(),
                "-e".into(),
                DRIVER.into(),
            ],
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

fn polynomial(
    p: &Problem,
    _pe: &dyn Fn(&Expr) -> Out,
    _list: &dyn Fn(&str) -> Out,
) -> Result<Code, Unsupported> {
    unsupported(format!("Symbolics has no {}", p.op.replace('_', " ")))
}

fn linear_algebra(p: &Problem, _pe: &dyn Fn(&Expr) -> Out, _setup: String) -> Result<Code, Unsupported> {
    unsupported(format!("Symbolics has no {}", p.op.replace('_', " ")))
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
    fn numbers_stay_exact() {
        assert_eq!(jl("x^(1/2) + 1/3"), "s_x^(1//2) + (1//3)");
        assert_eq!(jl("-1/2*x"), "-(1//2)*s_x");
        assert_eq!(jl("x/2 + 2/x"), "s_x/2 + 2/s_x");
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
    }

    #[test]
    fn functions_by_their_julia_names() {
        assert_eq!(jl("bessel_j(1/2, x)"), "besselj((1//2), s_x)");
        assert_eq!(jl("hankel_1(0, x)"), "hankelh1(0, s_x)");
        assert_eq!(jl("log(2, x) + atan2(y, x)"), "log(2, s_x) + atan(s_y, s_x)");
        assert_eq!(jl("log_gamma(x) + ei(x)"), "loggamma(s_x) + expinti(s_x)");
        assert_eq!(jl("ceiling(x) + re(x) + conjugate(x)"), "ceil(s_x) + real(s_x) + conj(s_x)");
        assert_eq!(jl("elliptic_k(x)"), "ellipk(s_x)");
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
        assert_eq!(c.task, "result = Symbolics.limit(1/s_x, s_x, 0, :right)");
        let c = code("limit", r#"{"expr": "1/x", "var": "x", "point": "-infinity"}"#);
        assert_eq!(c.task, "result = Symbolics.limit(1/s_x, s_x, -Inf)");
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
