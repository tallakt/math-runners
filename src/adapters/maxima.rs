//! Maxima, one session from a pipe, reset with `kill(all)` and `reset()`
//! before each problem. The answer is `string(result)` with `display2d:
//! false`.

use super::{
    Adapter, Condition, MonomialOrder, PolyDomain, conditions, monomial_order, poly_domain, quoted,
};
use crate::expr::{Expr, Rel};
use crate::print::{Dialect, Out, Printer, Unsupported, diff_args, unsupported};
use crate::problem::{Assumption, Problem};
use crate::session::Spec;

pub struct Maxima;

/// The forms the unknown functions take: as calls, `y(x)` (desolve), or as
/// plain symbols, `y` (ode2), or subscripted, `a[n]` (solve_rec).
#[derive(Clone, Copy, PartialEq)]
enum FnForm {
    Call,
    Symbol,
    Subscript,
}

struct D {
    funcs: Vec<(String, Vec<String>)>,
    form: FnForm,
}

impl Dialect for D {
    fn system(&self) -> &'static str {
        "Maxima"
    }

    fn symbol(&self, name: &str) -> Out {
        Ok(match name {
            "pi" => "%pi".into(),
            "e" => "%e".into(),
            "i" => "%i".into(),
            "infinity" => "inf".into(),
            "euler_gamma" => "%gamma".into(),
            "catalan" => "%catalan".into(),
            "undefined" => "und".into(),
            s => s.to_string(),
        })
    }

    fn minus_infinity(&self) -> Option<String> {
        Some("minf".into())
    }

    fn rel_op(&self, r: Rel) -> &'static str {
        match r {
            Rel::Ne => "#",
            r => r.neutral(),
        }
    }

    fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out {
        if name == "subs" {
            let (e, v, a) = crate::print::subs_args(args)?;
            return Ok(format!("at({}, {v} = {})", p.print(&e)?, p.print(&a)?));
        }
        if name == "diff" {
            let (f, vars) = diff_args(args)?;
            let vs: Vec<String> = vars.iter().map(|(v, k)| format!("{v}, {k}")).collect();
            let quote = if self.form == FnForm::Call { "" } else { "'" };
            return Ok(format!("{quote}diff({}, {})", p.print(&f)?, vs.join(", ")));
        }
        let a = p.args(args)?;
        let n = a.len();
        let f = |s: &str| Ok(format!("{s}({})", a.join(", ")));
        if self.funcs.iter().any(|f| f.0 == name) {
            return Ok(match self.form {
                FnForm::Call => format!("{name}({})", a.join(", ")),
                FnForm::Symbol => name.to_string(),
                FnForm::Subscript => format!("{name}[{}]", a.join(", ")),
            });
        }
        match (name, n) {
            (
                "exp" | "log" | "sqrt" | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "asin"
                | "acos" | "atan" | "acot" | "asec" | "acsc" | "sinh" | "cosh" | "tanh" | "coth"
                | "sech" | "csch" | "asinh" | "acosh" | "atanh" | "acoth" | "asech" | "acsch"
                | "abs" | "floor" | "ceiling" | "conjugate" | "factorial" | "gamma" | "log_gamma"
                | "erf" | "erfc" | "erfi" | "fresnel_s" | "fresnel_c" | "zeta" | "lambert_w"
                | "airy_ai" | "airy_bi",
                1,
            ) => f(name),
            ("log", 2) => Ok(format!("log({})/log({})", a[1], a[0])),
            ("root", 2) => Ok(format!("({})^(1/({}))", a[0], a[1])),
            (
                "atan2" | "binomial" | "pochhammer" | "beta" | "bessel_j" | "bessel_y" | "bessel_i"
                | "bessel_k" | "hankel_1" | "hankel_2" | "elliptic_f" | "elliptic_e" | "legendre_p"
                | "legendre_q" | "chebyshev_t" | "chebyshev_u" | "struve_h" | "struve_l",
                2,
            ) => f(name),
            ("sign", 1) => f("signum"),
            ("re", 1) => f("realpart"),
            ("im", 1) => f("imagpart"),
            ("arg", 1) => f("carg"),
            ("gamma", 2) => f("gamma_incomplete"),
            ("digamma", 1) => Ok(format!("psi[0]({})", a[0])),
            ("polygamma", 2) => Ok(format!("psi[{}]({})", a[0], a[1])),
            ("ei", 1) => f("expintegral_ei"),
            ("li", 1) => f("expintegral_li"),
            ("si", 1) => f("expintegral_si"),
            ("ci", 1) => f("expintegral_ci"),
            ("shi", 1) => f("expintegral_shi"),
            ("chi", 1) => f("expintegral_chi"),
            ("expint", 2) => f("expintegral_e"),
            ("polylog", 2) => Ok(format!("li[{}]({})", a[0], a[1])),
            ("lambert_w", 2) => f("generalized_lambert_w"),
            ("airy_ai_prime", 1) => f("airy_dai"),
            ("airy_bi_prime", 1) => f("airy_dbi"),
            ("elliptic_k", 1) => f("elliptic_kc"),
            ("elliptic_e", 1) => f("elliptic_ec"),
            ("elliptic_pi", 2) => Ok(format!("elliptic_pi({}, %pi/2, {})", a[0], a[1])),
            ("elliptic_pi", 3) => f("elliptic_pi"),
            ("hypergeometric", 3) => f("hypergeometric"),
            ("hermite_h", 2) => f("hermite"),
            ("laguerre_l", 2) => f("laguerre"),
            ("gegenbauer_c", 3) => f("ultraspherical"),
            ("jacobi_p", 4) => f("jacobi_p"),
            ("heaviside", 1) => f("unit_step"),
            ("dirac_delta", 1) => f("delta"),
            ("kronecker_delta", 2) => f("kron_delta"),
            ("zeta", 2) | ("lerch_phi", 3) | ("kelvin_ber", 2) | ("kelvin_bei", 2) => {
                unsupported(format!("Maxima has no {name} of {n} arguments"))
            }
            _ if !crate::functions::NEUTRAL.contains(&name) => f(name),
            _ => unsupported(format!("Maxima: no form for {name} of {n} arguments")),
        }
    }
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

struct Script {
    /// Before the problem starts: loads, assumptions, definitions.
    pre: Vec<String>,
    /// The task: its value is the answer.
    task: String,
}

fn build(p: &Problem) -> Result<Script, Unsupported> {
    let funcs = p.unknown_functions()?;
    let d = D {
        funcs: funcs.clone(),
        form: FnForm::Call,
    };
    let pr = Printer::new(&d);
    let pe = |e: &Expr| pr.print(e);
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
    let mut pre = Vec::new();
    for a in p.assumptions()? {
        match a {
            Assumption::Rel(e) => pre.push(format!("assume({})", pe(&e)?)),
            Assumption::Element(s, set) => match set.as_str() {
                "naturals" => {
                    pre.push(format!("declare({s}, integer)"));
                    pre.push(format!("assume({s} >= 0)"));
                }
                "integers" => pre.push(format!("declare({s}, integer)")),
                "rationals" => pre.push(format!("declare({s}, rational)")),
                "reals" => pre.push(format!("declare({s}, real)")),
                _ => pre.push(format!("declare({s}, complex)")),
            },
        }
    }
    let task = match p.op.as_str() {
        "integrate" => {
            let (f, x) = (arg("integrand")?, p.symbol("var")?);
            if p.has("from") || p.has("to") {
                format!("integrate({f}, {x}, {}, {})", arg("from")?, arg("to")?)
            } else {
                format!("integrate({f}, {x})")
            }
        }
        "solve_ode" => ode(p, &funcs, &mut pre)?,
        "solve_pde" => return unsupported("Maxima has no PDE solver"),
        "sum" => {
            pre.push("load(simplify_sum)".into());
            format!(
                "simplify_sum(sum({}, {}, {}, {}))",
                arg("term")?,
                p.symbol("var")?,
                arg("from")?,
                arg("to")?
            )
        }
        "product" => format!(
            "block([simpproduct: true], product({}, {}, {}, {}))",
            arg("term")?,
            p.symbol("var")?,
            arg("from")?,
            arg("to")?
        ),
        "solve_recurrence" => {
            let eqs = p.exprs("equations")?;
            if eqs.len() != 1 || funcs.len() != 1 {
                return unsupported("Maxima's solve_rec solves one recurrence");
            }
            let ds = D {
                funcs: funcs.clone(),
                form: FnForm::Subscript,
            };
            let ps = Printer::new(&ds);
            let mut parts = vec![ps.print(&eqs[0])?, ps.print(&p.expr("function")?)?];
            for c in p.exprs("conditions")? {
                parts.push(ps.print(&c)?);
            }
            pre.push("load(solve_rec)".into());
            format!("solve_rec({})", parts.join(", "))
        }
        "limit" => {
            let dir = match p.direction()? {
                Some('+') => ", plus",
                Some(_) => ", minus",
                None => "",
            };
            format!(
                "limit({}, {}, {}{dir})",
                arg("expr")?,
                p.symbol("var")?,
                arg("point")?
            )
        }
        "series" => format!(
            "taylor({}, {}, {}, {})",
            arg("expr")?,
            p.symbol("var")?,
            arg("point")?,
            p.int("order")? - 1
        ),
        "laplace" | "inverse_laplace" => {
            p.check_convention()?;
            let (f, x, y) = (arg("expr")?, p.symbol("var")?, p.symbol("transform_var")?);
            if p.op == "laplace" {
                format!("laplace({f}, {x}, {y})")
            } else {
                format!("ilt({f}, {x}, {y})")
            }
        }
        "fourier"
        | "inverse_fourier"
        | "mellin"
        | "inverse_mellin"
        | "z_transform"
        | "inverse_z_transform" => {
            return unsupported(format!("Maxima has no {}", p.op));
        }
        "solve" => {
            let eqs = p.exprs("equations")?;
            let inequality = eqs
                .iter()
                .any(|e| e.is_relation() && !matches!(e, Expr::Rel(Rel::Eq, ..)));
            let vars = format!("[{}]", p.symbols("vars")?.join(", "));
            if inequality {
                pre.push("load(fourier_elim)".into());
                format!("fourier_elim({}, {vars})", list("equations")?)
            } else if p.real_domain()?
                && eqs
                    .iter()
                    .all(|e| e.is_polynomial_in(&p.symbols("vars").unwrap_or_default()))
            {
                // solve takes no domain; algsys keeps the real solutions
                format!(
                    "block([realonly: true], algsys({}, {vars}))",
                    list("equations")?
                )
            } else {
                format!("solve({}, {vars})", list("equations")?)
            }
        }
        "simplify" => {
            // Maxima has no one simplifier: the shortest of its usual ones
            pre.push(
                "mr_simp(mr_e) := block([mr_c, mr_best], \
                 mr_c: [mr_e, ratsimp(mr_e), fullratsimp(mr_e), trigsimp(mr_e), \
                 trigsimp(fullratsimp(mr_e)), radcan(mr_e), factor(mr_e)], \
                 mr_best: first(mr_c), \
                 for mr_x in rest(mr_c) do if slength(string(mr_x)) < slength(string(mr_best)) \
                 then mr_best: mr_x, mr_best)"
                    .into(),
            );
            format!("mr_simp({})", arg("expr")?)
        }
        "evaluate" => format!("block([besselexpand: true], ev({}, nouns))", arg("expr")?),
        "hyperexpand" => {
            // autoloaded from a file of Maxima code, which kill(all) unloads
            pre.push("hypergeometric_simp(1)".into());
            format!("hypergeometric_simp({})", arg("expr")?)
        }
        "factor" => match poly_domain(p)? {
            PolyDomain::Rationals => format!("factor({})", arg("expr")?),
            PolyDomain::Gaussian => format!("gfactor({})", arg("expr")?),
        },
        "gcd" => format!(
            "lreduce(lambda([mr_a, mr_b], gcd(mr_a, mr_b)), {})",
            list("polynomials")?
        ),
        "lcm" => {
            pre.push("load(functs)".into());
            format!(
                "lreduce(lambda([mr_a, mr_b], lcm(mr_a, mr_b)), {})",
                list("polynomials")?
            )
        }
        "resultant" => {
            let ps = p.exprs("polynomials")?;
            if ps.len() != 2 {
                return unsupported("a resultant of other than two polynomials");
            }
            format!(
                "resultant({}, {}, {})",
                pe(&ps[0])?,
                pe(&ps[1])?,
                p.symbol("var")?
            )
        }
        "minimal_polynomial" => {
            return unsupported("Maxima has no minimal polynomial of an algebraic number");
        }
        "discriminant" => format!("poly_discriminant({}, {})", arg("expr")?, p.symbol("var")?),
        "partial_fractions" => format!("partfrac({}, {})", arg("expr")?, p.symbol("var")?),
        "groebner" => {
            let order = match monomial_order(p)? {
                MonomialOrder::Lex => "lex",
                MonomialOrder::Grlex => "grlex",
                MonomialOrder::Grevlex => "grevlex",
            };
            pre.push("load(grobner)".into());
            format!(
                "block([poly_monomial_order: {order}], poly_reduced_grobner({}, [{}]))",
                list("polynomials")?,
                p.symbols("vars")?.join(", ")
            )
        }
        // whether the relation holds for every value: `is`, true, false or unknown
        "holds" => match p.expr("relation")? {
            Expr::Rel(Rel::Eq, a, b) => format!("is(equal({}, {}))", pe(&a)?, pe(&b)?),
            Expr::Rel(Rel::Ne, a, b) => format!("is(notequal({}, {}))", pe(&a)?, pe(&b)?),
            r => format!("is({})", pe(&r)?),
        },
        // a power of a symbolic exponent: diag's function of the matrix, t^n
        "matrix_power" => {
            pre.push("load(diag)".into());
            format!(
                "mat_function(lambda([mr_t], mr_t^({})), apply(matrix, {}))",
                arg("power")?,
                arg("matrix")?
            )
        }
        "determinant" | "inverse" | "rank" | "nullspace" | "eigenvalues" | "eigenvectors"
        | "jordan_form" | "solve_linear" => {
            let mut m = format!("apply(matrix, {})", arg("matrix")?);
            if p.has("power") {
                pre.push("load(diag)".into());
                m = format!(
                    "mat_function(lambda([mr_t], mr_t^({})), {m})",
                    arg("power")?
                );
            }
            match p.op.as_str() {
                "determinant" if p.has("power") => format!("ratsimp(determinant({m}))"),
                "determinant" => format!("determinant({m})"),
                "inverse" => format!("invert({m})"),
                "rank" => format!("rank({m})"),
                // the basis vectors, out of span(...)
                "nullspace" => format!("args(nullspace({m}))"),
                "eigenvalues" => format!("eigenvalues({m})"),
                "eigenvectors" => format!("eigenvectors({m})"),
                "jordan_form" => {
                    pre.push("load(diag)".into());
                    format!("dispJordan(jordan({m}))")
                }
                _ => format!(
                    "block([mr_m: {m}, mr_b: {}, mr_v], \
                     mr_v: makelist(concat('mr_x, mr_i), mr_i, 1, length(mr_b)), \
                     map(rhs, linsolve(makelist(sum(mr_m[mr_i, mr_j]*mr_v[mr_j], mr_j, 1, length(mr_v)) = mr_b[mr_i], \
                     mr_i, 1, length(mr_b)), mr_v)))",
                    arg("rhs")?
                ),
            }
        }
        op => return unsupported(format!("the op {op} is not known to the runner")),
    };
    Ok(Script { pre, task })
}

/// An ODE: one equation of order one or two by `ode2`, with `ic1`, `ic2`
/// or `bc2` for the conditions; a system, or a higher order, by `desolve`,
/// with `atvalue` for the conditions.
fn ode(p: &Problem, funcs: &[(String, Vec<String>)], pre: &mut Vec<String>) -> Out {
    let eqs = p.exprs("equations")?;
    let conds = conditions(p, "conditions")?;
    if funcs.iter().any(|f| f.1.len() != 1) {
        return unsupported("an unknown function of several variables");
    }
    let x = funcs
        .first()
        .map(|f| f.1[0].clone())
        .ok_or(Unsupported("no function".into()))?;
    let n = eqs.iter().map(order).max().unwrap_or(0);
    if eqs.len() == 1 && funcs.len() == 1 && (1..=2).contains(&n) {
        let ds = D {
            funcs: funcs.to_vec(),
            form: FnForm::Symbol,
        };
        let ps = Printer::new(&ds);
        let y = &funcs[0].0;
        let mut sol = format!("ode2({}, {y}, {x})", ps.print(&eqs[0])?);
        let pc = Printer::new(&ds);
        let point = |c: &Condition| -> Out {
            if c.at.len() != 1 {
                return unsupported("a condition at more than one point");
            }
            pc.print(&c.at[0])
        };
        let value = |c: &Condition| pc.print(&c.value);
        match conds.as_slice() {
            [] => {}
            [c] if c.order == 0 && n == 1 => {
                sol = format!("ic1({sol}, {x} = {}, {y} = {})", point(c)?, value(c)?);
            }
            [a, b] if n == 2 => {
                let (a, b) = if a.order <= b.order { (a, b) } else { (b, a) };
                if a.order == 0 && b.order == 1 && point(a)? == point(b)? {
                    sol = format!(
                        "ic2({sol}, {x} = {}, {y} = {}, 'diff({y}, {x}) = {})",
                        point(a)?,
                        value(a)?,
                        value(b)?
                    );
                } else if a.order == 0 && b.order == 0 {
                    sol = format!(
                        "bc2({sol}, {x} = {}, {y} = {}, {x} = {}, {y} = {})",
                        point(a)?,
                        value(a)?,
                        point(b)?,
                        value(b)?
                    );
                } else {
                    return unsupported("conditions that ic2 and bc2 do not take");
                }
            }
            _ => return unsupported("conditions that ic1, ic2 and bc2 do not take"),
        }
        return Ok(sol);
    }
    // desolve, by the Laplace transform: the conditions at 0
    let d = D {
        funcs: funcs.to_vec(),
        form: FnForm::Call,
    };
    let pr = Printer::new(&d);
    for c in &conds {
        if c.at.len() != 1 {
            return unsupported("a condition at more than one point");
        }
        let at = pr.print(&c.at[0])?;
        let f = if c.order == 0 {
            format!("{}({x})", c.function)
        } else {
            format!("'diff({}({x}), {x}, {})", c.function, c.order)
        };
        pre.push(format!("atvalue({f}, {x} = {at}, {})", pr.print(&c.value)?));
    }
    let eqs: Vec<String> = eqs.iter().map(|e| pr.print(e)).collect::<Result<_, _>>()?;
    let fs: Vec<String> = funcs.iter().map(|(f, _)| format!("{f}({x})")).collect();
    Ok(format!(
        "desolve([{}], [{}])",
        eqs.join(", "),
        fs.join(", ")
    ))
}

impl Adapter for Maxima {
    fn name(&self) -> &'static str {
        "Maxima"
    }

    fn slug(&self) -> &'static str {
        "maxima"
    }

    fn default_command(&self) -> &'static str {
        "maxima"
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: program.to_string(),
            args: vec!["--very-quiet".into(), "--disable-readline".into()],
            env: vec![],
            dir: None,
        }
    }

    fn start(&self, nonce: &str) -> String {
        // Maxima loads some of itself at the first use of a command: done
        // here, so that it is not timed as the first problem's. (What is
        // Maxima code, `kill(all)` unloads again: those are loaded before
        // each problem that needs them, untimed.)
        format!(
            "display2d: false$ linel: 1000000$\n\
             errcatch(nullspace(matrix([1, 2], [2, 4])), eigenvectors(matrix([1, 2], [3, 4])), \
             desolve(diff(f(x), x) = f(x), f(x)), laplace(exp(t), t, s), ilt(1/(s + 1), s, t), \
             legendre_p(2, x), integrate(sin(x)^2, x), integrate(exp(-x^2), x, 0, inf), \
             limit(sin(x)/x, x, 0), taylor(exp(x), x, 0, 2), solve(x^2 = 1, x), \
             ode2('diff(y, x) = y, y, x), poly_discriminant(x^2 + 1, x), partfrac(1/(x^2 - 1), x), \
             rank(matrix([1, 2], [2, 4])), bessel_j(1/2, x), gamma_incomplete(1, x))$\n\
             kill(all)$ reset()$ display2d: false$ linel: 1000000$\n\
             printf(true, \"~%@@MR-VERSION ~a~%@@MR-READY ~a~%\", build_info()@version, {})$\n",
            quoted(nonce)
        )
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        let s = build(p)?;
        let n = quoted(nonce);
        let mut out = String::from("kill(all)$ reset()$ display2d: false$ linel: 1000000$\n");
        for line in &s.pre {
            out.push_str(&format!("{line}$\n"));
        }
        out.push_str(&format!(
            "printf(true, \"~%@@MR-BEGIN ~a~%\", {n})$\n\
             mr_c0: elapsed_run_time()$ mr_w0: elapsed_real_time()$\n\
             mr_r: errcatch({})$\n\
             mr_c1: elapsed_run_time()$ mr_w1: elapsed_real_time()$\n\
             printf(true, \"~%@@MR-TIME ~,6f ~,6f~%\", mr_c1 - mr_c0, mr_w1 - mr_w0)$\n\
             if mr_r = [] then printf(true, \"~%@@MR-ERROR~%\") else \
             printf(true, \"~%@@MR-ANSWER ~a~%~a~%@@MR-ANSWER-END ~a~%\", {n}, string(first(mr_r)), {n})$\n\
             printf(true, \"~%@@MR-END ~a~%\", {n})$\n",
            s.task
        ));
        Ok(out)
    }

    fn goodbye(&self) -> &'static str {
        "quit()$"
    }

    fn clean_noise(&self, text: &str) -> String {
        text.lines()
            .filter(|l| !l.contains("To debug this try: debugmode(true)"))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }
}
