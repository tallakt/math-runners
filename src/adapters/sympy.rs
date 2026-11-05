//! SymPy, driven by a small Python program inside SymPy
//! (`sympy_driver.py`), which runs the code this adapter writes for each
//! problem. The answer is `str(result)`, which `sympify` reads back.

use serde_json::json;

use super::{Adapter, MonomialOrder, PolyDomain, conditions, monomial_order, poly_domain};
use crate::expr::{Expr, Rel};
use crate::print::{Dialect, Out, Printer, Unsupported, diff_args, unsupported};
use crate::problem::{Assumption, Problem};
use crate::session::Spec;

pub struct SymPy;

const DRIVER: &str = include_str!("sympy_driver.py");

/// The Python name of a symbol of the problem.
fn var(name: &str) -> String {
    format!("s_{name}")
}

fn fun(name: &str) -> String {
    format!("f_{name}")
}

struct D {
    funcs: Vec<(String, Vec<String>)>,
}

impl Dialect for D {
    fn system(&self) -> &'static str {
        "SymPy"
    }

    fn int(&self, n: &str) -> String {
        format!("sp.Integer({n})")
    }

    fn dec(&self, d: &str) -> String {
        format!("sp.Float('{d}')")
    }

    fn symbol(&self, name: &str) -> Out {
        Ok(match name {
            "pi" => "sp.pi".into(),
            "e" => "sp.E".into(),
            "i" => "sp.I".into(),
            "infinity" => "sp.oo".into(),
            "euler_gamma" => "sp.EulerGamma".into(),
            "catalan" => "sp.Catalan".into(),
            "undefined" => "sp.nan".into(),
            s => var(s),
        })
    }

    fn pow_op(&self) -> &'static str {
        "**"
    }

    fn rel(&self, r: Rel, a: &str, b: &str) -> Option<String> {
        let f = match r {
            Rel::Eq => "Eq",
            Rel::Ne => "Ne",
            Rel::Lt => "Lt",
            Rel::Le => "Le",
            Rel::Gt => "Gt",
            Rel::Ge => "Ge",
        };
        Some(format!("sp.{f}({a}, {b})"))
    }

    fn logic_call(&self, op: &str, args: &[String]) -> Option<String> {
        let f = match op {
            "and" => "And",
            "or" => "Or",
            _ => "Not",
        };
        Some(format!("sp.{f}({})", args.join(", ")))
    }

    fn prime(&self, p: &Printer, name: &str, order: usize, args: &[Expr]) -> Out {
        let Some((_, vars)) = self.funcs.iter().find(|f| f.0 == name) else {
            return unsupported(format!("{name} is not an unknown function"));
        };
        if vars.len() != 1 || args.len() != 1 {
            return unsupported("a derivative condition of a function of several variables");
        }
        let x = var(&vars[0]);
        Ok(format!(
            "{}({x}).diff({x}, {order}).subs({x}, {})",
            fun(name),
            p.print(&args[0])?
        ))
    }

    fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out {
        if name == "subs" {
            let (e, v, a) = crate::print::subs_args(args)?;
            return Ok(format!(
                "({}).subs({}, {})",
                p.print(&e)?,
                var(&v),
                p.print(&a)?
            ));
        }
        if name == "diff" {
            let (f, vars) = diff_args(args)?;
            let vs: Vec<String> = vars
                .iter()
                .map(|(v, k)| format!("({}, {k})", var(v)))
                .collect();
            return Ok(format!(
                "sp.Derivative({}, {})",
                p.print(&f)?,
                vs.join(", ")
            ));
        }
        if name == "hypergeometric" && args.len() == 3 {
            return Ok(format!(
                "sp.hyper({}, {}, {})",
                p.print(&args[0])?,
                p.print(&args[1])?,
                p.print(&args[2])?
            ));
        }
        let a = p.args(args)?;
        let n = a.len();
        let f = |s: &str| Ok(format!("sp.{s}({})", a.join(", ")));
        match (name, n) {
            (
                "exp" | "sqrt" | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "asin" | "acos"
                | "atan" | "acot" | "asec" | "acsc" | "sinh" | "cosh" | "tanh" | "coth" | "sech"
                | "csch" | "asinh" | "acosh" | "atanh" | "acoth" | "asech" | "acsch" | "sign"
                | "floor" | "ceiling" | "re" | "im" | "arg" | "conjugate" | "factorial" | "gamma"
                | "digamma" | "erf" | "erfc" | "erfi" | "li" | "zeta" | "elliptic_k" | "elliptic_e",
                1,
            ) => f(name),
            ("log", 1) => f("log"),
            ("log", 2) => Ok(format!("sp.log({}, {})", a[1], a[0])),
            ("root", 2) => Ok(format!("sp.root({}, {})", a[0], a[1])),
            ("atan2", 2) => f("atan2"),
            ("abs", 1) => f("Abs"),
            (
                "binomial" | "polygamma" | "beta" | "expint" | "polylog" | "zeta" | "elliptic_f"
                | "elliptic_e" | "elliptic_pi",
                2,
            ) => f(name),
            ("pochhammer", 2) => f("RisingFactorial"),
            ("gamma", 2) => f("uppergamma"),
            ("log_gamma", 1) => f("loggamma"),
            ("fresnel_s", 1) => f("fresnels"),
            ("fresnel_c", 1) => f("fresnelc"),
            ("ei", 1) => f("Ei"),
            ("si", 1) => f("Si"),
            ("ci", 1) => f("Ci"),
            ("shi", 1) => f("Shi"),
            ("chi", 1) => f("Chi"),
            ("lerch_phi", 3) => f("lerchphi"),
            ("lambert_w", 1) => f("LambertW"),
            ("lambert_w", 2) => Ok(format!("sp.LambertW({}, {})", a[1], a[0])),
            ("bessel_j", 2) => f("besselj"),
            ("bessel_y", 2) => f("bessely"),
            ("bessel_i", 2) => f("besseli"),
            ("bessel_k", 2) => f("besselk"),
            ("hankel_1", 2) => f("hankel1"),
            ("hankel_2", 2) => f("hankel2"),
            ("airy_ai", 1) => f("airyai"),
            ("airy_bi", 1) => f("airybi"),
            ("airy_ai_prime", 1) => f("airyaiprime"),
            ("airy_bi_prime", 1) => f("airybiprime"),
            ("elliptic_pi", 3) => f("elliptic_pi"),
            ("legendre_p", 2) => f("legendre"),
            ("hermite_h", 2) => f("hermite"),
            ("laguerre_l", 2) => f("laguerre"),
            ("chebyshev_t", 2) => f("chebyshevt"),
            ("chebyshev_u", 2) => f("chebyshevu"),
            ("gegenbauer_c", 3) => f("gegenbauer"),
            ("jacobi_p", 4) => f("jacobi"),
            ("heaviside", 1) => f("Heaviside"),
            ("dirac_delta", 1) => f("DiracDelta"),
            ("kronecker_delta", 2) => f("KroneckerDelta"),
            ("legendre_q" | "struve_h" | "struve_l" | "kelvin_ber" | "kelvin_bei", _) => {
                unsupported(format!("SymPy has no {name}"))
            }
            _ if self.funcs.iter().any(|f| f.0 == name) || !is_known(name) => {
                Ok(format!("{}({})", fun(name), a.join(", ")))
            }
            _ => unsupported(format!("SymPy: no form for {name} of {n} arguments")),
        }
    }
}

/// The names of the neutral syntax's functions.
pub fn is_known(name: &str) -> bool {
    crate::functions::NEUTRAL.contains(&name)
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

    // the symbols, with what is assumed of them
    let assumptions = p.assumptions()?;
    let mut props: Vec<(String, Vec<&str>)> = p
        .all_symbols()?
        .into_iter()
        .map(|s| (s, Vec::new()))
        .collect();
    let mut refine = Vec::new();
    for a in &assumptions {
        let mut put = |s: &str, prop: &'static str| {
            if let Some(entry) = props.iter_mut().find(|e| e.0 == s) {
                entry.1.push(prop);
            }
        };
        match a {
            Assumption::Element(s, set) => match set.as_str() {
                "naturals" => {
                    put(s, "integer");
                    put(s, "nonnegative")
                }
                "integers" => put(s, "integer"),
                "rationals" => put(s, "rational"),
                "reals" => put(s, "real"),
                _ => {}
            },
            Assumption::Rel(e) => match a.sign() {
                Some((s, prop)) if prop != "zero" => put(&s, prop),
                _ => refine.push(pe(e)?),
            },
        }
    }
    // the variable of integration and of a limit is real, of a sum an
    // integer (formats, section 3)
    let real_var = match p.op.as_str() {
        "integrate" | "limit" => Some((p.symbol("var")?, "real")),
        "sum" | "product" => Some((p.symbol("var")?, "integer")),
        "solve" if p.real_domain()? => None,
        _ => None,
    };
    if let Some((v, prop)) = &real_var {
        if let Some(e) = props.iter_mut().find(|e| e.0 == *v) {
            if e.1.is_empty() {
                e.1.push(prop);
            }
        }
    }
    if matches!(p.op.as_str(), "solve" | "holds") && p.real_domain()? {
        for v in p.symbols("vars")? {
            if let Some(e) = props.iter_mut().find(|e| e.0 == v) {
                if e.1.is_empty() {
                    e.1.push("real");
                }
            }
        }
    }
    let mut setup = String::new();
    for (s, ps) in &props {
        let kw: String = ps.iter().map(|q| format!(", {q}=True")).collect();
        setup.push_str(&format!("{} = sp.Symbol('{s}'{kw})\n", var(s)));
    }
    for (f, _) in &funcs {
        setup.push_str(&format!("{} = sp.Function('{f}')\n", fun(f)));
    }
    let mut answer = "result".to_string();
    let task = match p.op.as_str() {
        "integrate" => {
            let (f, x) = (arg("integrand")?, var(&p.symbol("var")?));
            if p.has("from") || p.has("to") {
                format!(
                    "result = sp.integrate({f}, ({x}, {}, {}))",
                    arg("from")?,
                    arg("to")?
                )
            } else {
                format!("result = sp.integrate({f}, {x})")
            }
        }
        "solve_ode" => {
            let eqs = p.exprs("equations")?;
            let fs: Vec<String> = funcs
                .iter()
                .map(|(f, vs)| {
                    format!(
                        "{}({})",
                        fun(f),
                        vs.iter().map(|v| var(v)).collect::<Vec<_>>().join(", ")
                    )
                })
                .collect();
            let ics = ics(p, &pr)?;
            if eqs.len() == 1 && fs.len() == 1 {
                format!("result = sp.dsolve({}, {}, ics={ics})", pe(&eqs[0])?, fs[0])
            } else {
                format!(
                    "result = sp.dsolve({}, [{}], ics={ics})",
                    list("equations")?,
                    fs.join(", ")
                )
            }
        }
        "solve_pde" => {
            if !p.exprs("conditions")?.is_empty() {
                return unsupported("SymPy's pdsolve takes no conditions");
            }
            let fs = &funcs.first().ok_or(Unsupported("no function".into()))?;
            let call = format!(
                "{}({})",
                fun(&fs.0),
                fs.1.iter().map(|v| var(v)).collect::<Vec<_>>().join(", ")
            );
            format!("result = sp.pdsolve({}, {call})", arg("equation")?)
        }
        "sum" | "product" => {
            let f = if p.op == "sum" {
                "summation"
            } else {
                "product"
            };
            format!(
                "result = sp.{f}({}, ({}, {}, {}))",
                arg("term")?,
                var(&p.symbol("var")?),
                arg("from")?,
                arg("to")?
            )
        }
        "solve_recurrence" => {
            let eqs = p.exprs("equations")?;
            if eqs.len() != 1 || funcs.len() != 1 {
                return unsupported("SymPy's rsolve solves one recurrence");
            }
            let (f, vs) = &funcs[0];
            let call = format!(
                "{}({})",
                fun(f),
                vs.iter().map(|v| var(v)).collect::<Vec<_>>().join(", ")
            );
            let mut init = Vec::new();
            for c in p.exprs("conditions")? {
                match c {
                    Expr::Rel(Rel::Eq, a, b) => init.push(format!("{}: {}", pe(&a)?, pe(&b)?)),
                    _ => return unsupported("a condition is not an equation"),
                }
            }
            format!(
                "result = sp.rsolve({}, {call}, {{{}}})",
                pe(&eqs[0])?,
                init.join(", ")
            )
        }
        "limit" => {
            let point = p.expr("point")?;
            let infinite = matches!(&point, Expr::Sym(s) if s == "infinity")
                || matches!(&point, Expr::Neg(a) if a.as_sym() == Some("infinity"));
            let dir = match (p.direction()?, infinite) {
                (_, true) => String::new(),
                (Some(c), _) => format!(", dir='{c}'"),
                (None, _) => ", dir='+-'".into(),
            };
            format!(
                "result = sp.limit({}, {}, {}{dir})",
                arg("expr")?,
                var(&p.symbol("var")?),
                pe(&point)?
            )
        }
        "series" => {
            answer = "result.removeO()".into();
            format!(
                "result = sp.series({}, {}, {}, {})",
                arg("expr")?,
                var(&p.symbol("var")?),
                arg("point")?,
                p.int("order")?
            )
        }
        "laplace"
        | "inverse_laplace"
        | "fourier"
        | "inverse_fourier"
        | "mellin"
        | "inverse_mellin"
        | "z_transform"
        | "inverse_z_transform" => {
            p.check_convention()?;
            let (f, x, y) = (
                arg("expr")?,
                var(&p.symbol("var")?),
                var(&p.symbol("transform_var")?),
            );
            match p.op.as_str() {
                "laplace" => format!("result = sp.laplace_transform({f}, {x}, {y}, noconds=True)"),
                "inverse_laplace" => {
                    format!("result = sp.inverse_laplace_transform({f}, {x}, {y})")
                }
                // SymPy's transform is of exp(-2*pi*i*x*k): at k = w/(2*pi)
                // it is the formats' one
                "fourier" => format!(
                    "k_ = sp.Dummy('k')\nresult = sp.fourier_transform({f}, {x}, k_).subs(k_, {y}/(2*sp.pi))"
                ),
                "inverse_fourier" => format!(
                    "k_ = sp.Dummy('k')\nresult = sp.inverse_fourier_transform(({f}).subs({x}, 2*sp.pi*k_), k_, {y})"
                ),
                "mellin" => format!("result = sp.mellin_transform({f}, {x}, {y}, noconds=True)"),
                "inverse_mellin" => {
                    format!("result = sp.inverse_mellin_transform({f}, {x}, {y}, (-sp.oo, sp.oo))")
                }
                _ => return unsupported("SymPy has no z-transform"),
            }
        }
        "solve" => solve(p, &pr)?,
        "simplify" => {
            let e = arg("expr")?;
            if refine.is_empty() {
                format!("result = sp.simplify({e})")
            } else {
                format!(
                    "result = sp.refine(sp.simplify({e}), sp.And({}))",
                    refine.join(", ")
                )
            }
        }
        "evaluate" => format!(
            "result = sp.simplify(sp.expand_func(sp.sympify({}).doit()))",
            arg("expr")?
        ),
        "hyperexpand" => format!("result = sp.hyperexpand({})", arg("expr")?),
        "factor" => {
            let g = if poly_domain(p)? == PolyDomain::Gaussian {
                ", gaussian=True"
            } else {
                ""
            };
            format!("result = sp.factor({}{g})", arg("expr")?)
        }
        "gcd" | "lcm" => format!("result = sp.{}_list({})", p.op, list("polynomials")?),
        "resultant" => {
            let ps = p.exprs("polynomials")?;
            if ps.len() != 2 {
                return unsupported("a resultant of other than two polynomials");
            }
            format!(
                "result = sp.resultant({}, {}, {})",
                pe(&ps[0])?,
                pe(&ps[1])?,
                var(&p.symbol("var")?)
            )
        }
        "minimal_polynomial" => format!(
            "result = sp.minimal_polynomial({}, {})",
            arg("expr")?,
            var(&p.symbol("var")?)
        ),
        "discriminant" => format!(
            "result = sp.discriminant({}, {})",
            arg("expr")?,
            var(&p.symbol("var")?)
        ),
        "partial_fractions" => format!(
            "result = sp.apart({}, {})",
            arg("expr")?,
            var(&p.symbol("var")?)
        ),
        "groebner" => {
            let order = match monomial_order(p)? {
                MonomialOrder::Lex => "lex",
                MonomialOrder::Grlex => "grlex",
                MonomialOrder::Grevlex => "grevlex",
            };
            let vars: Vec<String> = p.symbols("vars")?.iter().map(|v| var(v)).collect();
            answer = "list(result.exprs)".into();
            format!(
                "result = sp.groebner({}, {}, order='{order}')",
                list("polynomials")?,
                vars.join(", ")
            )
        }
        // whether the relation holds for every value: ask of the sides' difference, True, False
        // or None
        "holds" => {
            let (rel, a, b) = match p.expr("relation")? {
                Expr::Rel(r, a, b) => (r, a, b),
                _ => return unsupported("the relation is not a relation"),
            };
            let q = match rel {
                Rel::Ge => "nonnegative",
                Rel::Gt => "positive",
                Rel::Le => "nonpositive",
                Rel::Lt => "negative",
                Rel::Eq => "zero",
                Rel::Ne => "nonzero",
            };
            format!(
                "d_ = sp.simplify(({}) - ({}))\nresult = sp.ask(sp.Q.{q}(d_))",
                pe(&a)?,
                pe(&b)?
            )
        }
        "matrix_power" => format!(
            "result = sp.Matrix({})**({})",
            arg("matrix")?,
            arg("power")?
        ),
        "determinant" if p.has("power") => format!(
            "result = sp.simplify((sp.Matrix({})**({})).det())",
            arg("matrix")?,
            arg("power")?
        ),
        "determinant" | "inverse" | "rank" | "nullspace" | "eigenvalues" | "eigenvectors"
        | "jordan_form" | "solve_linear" => {
            let m = format!("sp.Matrix({})", arg("matrix")?);
            match p.op.as_str() {
                "determinant" => format!("result = {m}.det()"),
                "inverse" => format!("result = {m}.inv()"),
                "rank" => format!("result = {m}.rank()"),
                "nullspace" => format!("result = {m}.nullspace()"),
                "eigenvalues" => format!("result = {m}.eigenvals(multiple=True)"),
                "eigenvectors" => format!("result = {m}.eigenvects()"),
                "jordan_form" => {
                    answer = "result[1]".into();
                    format!("result = {m}.jordan_form()")
                }
                _ => {
                    // the solution vector, its free parameters tau0, tau1, ...
                    answer = "result[0]".into();
                    format!(
                        "result = {m}.gauss_jordan_solve(sp.Matrix({}))",
                        arg("rhs")?
                    )
                }
            }
        }
        op => return unsupported(format!("the op {op} is not known to the runner")),
    };
    Ok(Code {
        setup,
        task,
        answer,
    })
}

/// The initial conditions of `dsolve`, as a dict.
fn ics(p: &Problem, pr: &Printer) -> Out {
    let mut out = Vec::new();
    for c in p.exprs("conditions")? {
        match c {
            Expr::Rel(Rel::Eq, a, b) => out.push(format!("{}: {}", pr.print(&a)?, pr.print(&b)?)),
            _ => return unsupported("a condition is not an equation"),
        }
    }
    // checked for their form, as the other adapters read them
    conditions(p, "conditions")?;
    Ok(format!("{{{}}}", out.join(", ")))
}

fn solve(p: &Problem, pr: &Printer) -> Out {
    let eqs = p.exprs("equations")?;
    let vars: Vec<String> = p.symbols("vars")?.iter().map(|v| var(v)).collect();
    let real = p.real_domain()?;
    let printed = eqs
        .iter()
        .map(|e| pr.print(e))
        .collect::<Result<Vec<_>, _>>()?;
    let inequality = eqs
        .iter()
        .any(|e| e.is_relation() && !matches!(e, Expr::Rel(Rel::Eq, ..)));
    if inequality {
        return Ok(format!(
            "result = sp.reduce_inequalities([{}], [{}])",
            printed.join(", "),
            vars.join(", ")
        ));
    }
    if eqs.len() == 1 && vars.len() == 1 {
        let domain = if real { "sp.S.Reals" } else { "sp.S.Complexes" };
        return Ok(format!(
            "result = sp.solveset({}, {}, {domain})",
            printed[0], vars[0]
        ));
    }
    Ok(format!(
        "result = sp.solve([{}], [{}], dict=True)",
        printed.join(", "),
        vars.join(", ")
    ))
}

impl Adapter for SymPy {
    fn name(&self) -> &'static str {
        "SymPy"
    }

    fn slug(&self) -> &'static str {
        "sympy"
    }

    fn default_command(&self) -> &'static str {
        "python3"
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: program.to_string(),
            args: vec!["-u".into(), "-c".into(), DRIVER.into()],
            env: vec![("PYTHONHASHSEED".into(), "0".into())],
            dir: None,
        }
    }

    fn start(&self, nonce: &str) -> String {
        json!({"nonce": nonce, "start": true}).to_string()
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        let c = build(p)?;
        Ok(
            json!({"nonce": nonce, "setup": c.setup, "task": c.task, "answer": c.answer})
                .to_string(),
        )
    }

    fn goodbye(&self) -> &'static str {
        "{\"nonce\": \"0\", \"quit\": true}"
    }
}

/// The code the adapter writes for a problem, for `math-run translate`.
pub fn show(p: &Problem) -> Result<String, Unsupported> {
    let c = build(p)?;
    Ok(format!("{}{}\nanswer = {}", c.setup, c.task, c.answer))
}
