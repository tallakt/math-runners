//! Giac, one session of its command-line program (`giac --sage`, the same
//! as `icas`) from a pipe, reset with `restart` before each problem. The
//! answer is `string(result)`.
//!
//! Giac prints its prompts and echoes the input on its standard output,
//! and `print` writes to its standard error: the adapter starts it with the
//! two joined (`2>&1`), and the protocol lines are printed with `print`.
//!
//! Giac has no clock to read: its `time(expr)` evaluates `expr` again and
//! again, for an average. The runner takes the times itself, from the line
//! that Giac prints as the work is done (`@@MR-COMPUTED`), the CPU time
//! from the operating system.

use super::{Adapter, MonomialOrder, PolyDomain, conditions, monomial_order, poly_domain, quoted};
use crate::expr::{Expr, Rel};
use crate::print::{Dialect, Out, Printer, Unsupported, diff_args, unsupported};
use crate::problem::{Assumption, Problem};
use crate::session::Spec;

pub struct Giac;

struct D {
    funcs: Vec<(String, Vec<String>)>,
}

impl Dialect for D {
    fn system(&self) -> &'static str {
        "Giac"
    }

    fn symbol(&self, name: &str) -> Out {
        Ok(match name {
            "pi" => "pi".into(),
            "e" => "e".into(),
            "i" => "i".into(),
            "infinity" => "inf".into(),
            "euler_gamma" => "euler_gamma".into(),
            "catalan" => "catalan".into(),
            "undefined" => "undef".into(),
            s => s.to_string(),
        })
    }

    fn minus_infinity(&self) -> Option<String> {
        Some("-inf".into())
    }

    fn prime(&self, p: &Printer, name: &str, order: usize, args: &[Expr]) -> Out {
        Ok(format!(
            "{name}{}({})",
            "'".repeat(order),
            p.args(args)?.join(", ")
        ))
    }

    fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out {
        if name == "diff" {
            let (f, vars) = diff_args(args)?;
            let vs: Vec<String> = vars
                .iter()
                .map(|(v, k)| {
                    if *k == 1 {
                        v.clone()
                    } else {
                        format!("{v}${k}")
                    }
                })
                .collect();
            return Ok(format!("diff({}, {})", p.print(&f)?, vs.join(", ")));
        }
        let a = p.args(args)?;
        let n = a.len();
        let f = |s: &str| Ok(format!("{s}({})", a.join(", ")));
        if self.funcs.iter().any(|f| f.0 == name) || !crate::functions::NEUTRAL.contains(&name) {
            return Ok(format!("{name}({})", a.join(", ")));
        }
        match (name, n) {
            (
                "exp" | "sqrt" | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "asin" | "acos"
                | "atan" | "acot" | "asec" | "acsc" | "sinh" | "cosh" | "tanh" | "coth" | "sech"
                | "csch" | "asinh" | "acosh" | "atanh" | "acoth" | "asech" | "acsch" | "abs"
                | "sign" | "floor" | "re" | "im" | "arg" | "factorial" | "erf" | "erfc" | "erfi",
                1,
            ) => f(name),
            ("log", 1) => f("ln"),
            ("log", 2) => Ok(format!("ln({})/ln({})", a[1], a[0])),
            ("root", 2) => Ok(format!("({})^(1/({}))", a[0], a[1])),
            ("atan2", 2) => f("atan2"),
            ("ceiling", 1) => f("ceil"),
            ("conjugate", 1) => f("conj"),
            ("binomial" | "pochhammer", 2) => f(name),
            ("gamma", 1) => f("Gamma"),
            ("gamma", 2) => f("ugamma"),
            ("log_gamma", 1) => f("lgamma"),
            ("digamma", 1) => f("Psi"),
            ("polygamma", 2) => Ok(format!("Psi({}, {})", a[1], a[0])),
            ("beta", 2) => f("Beta"),
            ("ei", 1) => f("Ei"),
            ("li", 1) => f("Li"),
            ("si", 1) => f("Si"),
            ("ci", 1) => f("Ci"),
            ("zeta", 1) => f("Zeta"),
            ("lambert_w", 1) => f("LambertW"),
            ("lambert_w", 2) => Ok(format!("LambertW({}, {})", a[1], a[0])),
            ("bessel_j", 2) => f("BesselJ"),
            ("bessel_y", 2) => f("BesselY"),
            ("bessel_i", 2) => f("BesselI"),
            ("bessel_k", 2) => f("BesselK"),
            ("hankel_1", 2) => Ok(format!(
                "(BesselJ({0}, {1}) + i*BesselY({0}, {1}))",
                a[0], a[1]
            )),
            ("hankel_2", 2) => Ok(format!(
                "(BesselJ({0}, {1}) - i*BesselY({0}, {1}))",
                a[0], a[1]
            )),
            ("airy_ai", 1) => f("Airy_Ai"),
            ("airy_bi", 1) => f("Airy_Bi"),
            ("hypergeometric", 3) => f("hyper"),
            ("legendre_p", 2) => f("legendre"),
            ("hermite_h", 2) => f("hermite"),
            // laguerre(n, x, a) is the generalized one, a = 0 when left out
            ("laguerre_l", 2) => f("laguerre"),
            ("chebyshev_t", 2) => f("tchebyshev1"),
            ("chebyshev_u", 2) => f("tchebyshev2"),
            ("heaviside", 1) => f("Heaviside"),
            ("dirac_delta", 1) => f("Dirac"),
            _ => unsupported(format!("Giac has no {name} of {n} arguments")),
        }
    }
}

struct Script {
    pre: Vec<String>,
    task: String,
    /// The answer, of `mrr`, the task's value.
    answer: String,
}

fn build(p: &Problem) -> Result<Script, Unsupported> {
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
    let mut pre = Vec::new();
    for a in p.assumptions()? {
        match a {
            Assumption::Rel(e) => pre.push(format!("assume({})", pe(&e)?)),
            Assumption::Element(s, set) => match set.as_str() {
                "naturals" => pre.push(format!("assume({s}, integer); additionally({s} >= 0)")),
                "integers" => pre.push(format!("assume({s}, integer)")),
                "rationals" => pre.push(format!("assume({s}, rational)")),
                "reals" => pre.push(format!("assume({s}, real)")),
                _ => pre.push(format!("assume({s}, complex)")),
            },
        }
    }
    let mut answer = "string(mrr)".to_string();
    let var = |k: &str| p.symbol(k);
    let task = match p.op.as_str() {
        "integrate" => {
            let (f, x) = (arg("integrand")?, var("var")?);
            if p.has("from") || p.has("to") {
                format!("integrate({f}, {x}, {}, {})", arg("from")?, arg("to")?)
            } else {
                format!("integrate({f}, {x})")
            }
        }
        "solve_ode" => {
            let eqs = p.exprs("equations")?;
            if eqs.len() != 1 || funcs.len() != 1 || funcs[0].1.len() != 1 {
                return unsupported("the runner gives Giac's desolve one ODE");
            }
            conditions(p, "conditions")?;
            let (y, x) = (&funcs[0].0, &funcs[0].1[0]);
            let conds = p.exprs("conditions")?;
            if conds.is_empty() {
                format!("desolve({}, {x}, {y})", pe(&eqs[0])?)
            } else {
                let mut all = vec![pe(&eqs[0])?];
                for c in &conds {
                    all.push(pe(c)?);
                }
                format!("desolve([{}], {x}, {y})", all.join(", "))
            }
        }
        "solve_pde" => return unsupported("Giac has no PDE solver"),
        "sum" | "product" => format!(
            "{}({}, {}, {}, {})",
            p.op,
            arg("term")?,
            var("var")?,
            arg("from")?,
            arg("to")?
        ),
        "solve_recurrence" => {
            let eqs = p.exprs("equations")?;
            if eqs.len() != 1 || funcs.len() != 1 {
                return unsupported("the runner gives Giac's rsolve one recurrence");
            }
            let mut parts = vec![pe(&eqs[0])?, arg("function")?];
            for c in p.exprs("conditions")? {
                parts.push(pe(&c)?);
            }
            format!("rsolve({})", parts.join(", "))
        }
        "limit" => {
            let dir = match p.direction()? {
                Some('+') => ", 1",
                Some(_) => ", -1",
                None => "",
            };
            format!(
                "limit({}, {}, {}{dir})",
                arg("expr")?,
                var("var")?,
                arg("point")?
            )
        }
        "series" => format!(
            "convert(series({}, {} = {}, {}), polynom)",
            arg("expr")?,
            var("var")?,
            arg("point")?,
            p.int("order")? - 1
        ),
        "laplace"
        | "inverse_laplace"
        | "fourier"
        | "inverse_fourier"
        | "z_transform"
        | "inverse_z_transform" => {
            p.check_convention()?;
            let f = match p.op.as_str() {
                "laplace" => "laplace",
                "inverse_laplace" => "ilaplace",
                "fourier" => "fourier",
                "inverse_fourier" => "ifourier",
                "z_transform" => "ztrans",
                _ => "invztrans",
            };
            format!(
                "{f}({}, {}, {})",
                arg("expr")?,
                var("var")?,
                var("transform_var")?
            )
        }
        "mellin" | "inverse_mellin" => return unsupported(format!("Giac has no {}", p.op)),
        "solve" => {
            let vars = format!("[{}]", p.symbols("vars")?.join(", "));
            let eqs = p.exprs("equations")?;
            let inequality = eqs
                .iter()
                .any(|e| e.is_relation() && !matches!(e, Expr::Rel(Rel::Eq, ..)));
            // solve is over the reals, csolve over the complex numbers
            let f = if inequality || p.real_domain()? {
                "solve"
            } else {
                "csolve"
            };
            format!("{f}({}, {vars})", list("equations")?)
        }
        "simplify" | "evaluate" => format!("simplify({})", arg("expr")?),
        "hyperexpand" => return unsupported("Giac has no hypergeometric expansion"),
        "factor" => match poly_domain(p)? {
            PolyDomain::Rationals => format!("factor({})", arg("expr")?),
            PolyDomain::Gaussian => format!("cfactor({})", arg("expr")?),
        },
        "gcd" | "lcm" => {
            let ps = p.exprs("polynomials")?;
            let mut acc = pe(ps.first().ok_or(Unsupported("no polynomials".into()))?)?;
            for q in &ps[1..] {
                acc = format!("{}({acc}, {})", p.op, pe(q)?);
            }
            acc
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
                var("var")?
            )
        }
        "discriminant" => format!("discriminant({}, {})", arg("expr")?, var("var")?),
        "partial_fractions" => format!("partfrac({}, {})", arg("expr")?, var("var")?),
        "groebner" => {
            let order = match monomial_order(p)? {
                MonomialOrder::Lex => "plex",
                MonomialOrder::Grlex => "tdeg",
                MonomialOrder::Grevlex => "revlex",
            };
            format!(
                "gbasis({}, [{}], {order})",
                list("polynomials")?,
                p.symbols("vars")?.join(", ")
            )
        }
        "determinant" | "inverse" | "rank" | "nullspace" | "eigenvalues" | "eigenvectors"
        | "jordan_form" | "solve_linear" => {
            let m = arg("matrix")?;
            match p.op.as_str() {
                "determinant" => format!("det({m})"),
                "inverse" => format!("inv({m})"),
                "rank" => format!("rank({m})"),
                "nullspace" => format!("ker({m})"),
                "eigenvalues" => format!("[eigenvals({m})]"),
                "eigenvectors" => format!("eigenvects({m})"),
                "jordan_form" => {
                    // jordan gives [P, J]: the form is J
                    answer = "string(mrr[1])".into();
                    format!("jordan({m})")
                }
                _ => format!("linsolve({m}, {})", arg("rhs")?),
            }
        }
        op => return unsupported(format!("the op {op} is not known to the runner")),
    };
    Ok(Script { pre, task, answer })
}

fn say(text: &str) -> String {
    format!("print({});", quoted(text))
}

impl Adapter for Giac {
    fn clean_version(&self, text: &str) -> String {
        // "giac 1.9.0, (c) B. Parisse and R. De Graeve, ...": the version
        let first = text
            .split(',')
            .next()
            .unwrap_or(text)
            .trim()
            .trim_matches('"');
        first
            .strip_prefix("giac ")
            .unwrap_or(first)
            .trim()
            .to_string()
    }

    fn name(&self) -> &'static str {
        "Giac"
    }

    fn slug(&self) -> &'static str {
        "giac"
    }

    fn default_command(&self) -> &'static str {
        "giac"
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "exec \"$0\" --sage 2>&1".into(),
                program.to_string(),
            ],
            env: vec![],
            dir: None,
        }
    }

    fn start(&self, nonce: &str) -> String {
        format!(
            "integrate(sin(x)^2, x); limit(sin(x)/x, x, 0); restart;\n\
             print(\"@@MR-VERSION \" + version());\n\
             {}\n",
            say(&format!("@@MR-READY {nonce}"))
        )
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        let s = build(p)?;
        let mut out = String::from("restart;\n");
        for l in &s.pre {
            out.push_str(&format!("{l};\n"));
        }
        out.push_str(&format!(
            "{}\n\
             mrr:={}; {} {} print({}); {}\n\
             {}\n",
            say(&format!("@@MR-BEGIN {nonce}")),
            s.task,
            say(&format!("@@MR-COMPUTED {nonce}")),
            say(&format!("@@MR-ANSWER {nonce}")),
            s.answer,
            say(&format!("@@MR-ANSWER-END {nonce}")),
            say(&format!("@@MR-END {nonce}")),
        ));
        Ok(out)
    }

    fn goodbye(&self) -> &'static str {
        "quit;"
    }

    fn clean_noise(&self, text: &str) -> String {
        // the prompts and echoes, and the values of the protocol's lines
        let mut out = Vec::new();
        for l in text.lines() {
            let t = l.trim();
            let prompt = t
                .split_once(">> ")
                .is_some_and(|(n, _)| n.bytes().all(|b| b.is_ascii_digit()));
            if t.is_empty() || prompt || t == "0" || t.starts_with("// ") || t.ends_with(",0") {
                continue;
            }
            out.push(t.trim_matches('"').to_string());
        }
        out.join(" ")
    }
}
