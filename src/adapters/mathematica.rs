//! Mathematica (the Wolfram Language kernel), one session of its
//! command-line kernel (`wolfram -noprompt`, also `math` or `WolframKernel`)
//! from a pipe, reset with `ClearAll["Global`*"]` and `ClearSystemCache[]`
//! before each problem. The answer is `ToString[result, InputForm]`.
//!
//! Untested: written from the Wolfram Language documentation, without a
//! kernel to run it on.

use super::{Adapter, MonomialOrder, PolyDomain, conditions, monomial_order, poly_domain, quoted};
use crate::expr::{Expr, Rel};
use crate::print::{Dialect, Out, Printer, Unsupported, diff_args, unsupported};
use crate::problem::{Assumption, Problem};
use crate::session::Spec;

pub struct Mathematica;

struct D {
    funcs: Vec<(String, Vec<String>)>,
}

/// A symbol: Mathematica's names have no `_` (it makes patterns), and the
/// single capitals are its own (`C`, `D`, `E`, `I`, `N`, `O`).
fn ident(name: &str) -> String {
    let s = name.replace('_', "$");
    if ["C", "D", "E", "I", "N", "O", "K"].contains(&s.as_str()) {
        format!("mr{s}")
    } else {
        s
    }
}

impl Dialect for D {
    fn system(&self) -> &'static str {
        "Mathematica"
    }

    fn symbol(&self, name: &str) -> Out {
        Ok(match name {
            "pi" => "Pi".into(),
            "e" => "E".into(),
            "i" => "I".into(),
            "infinity" => "Infinity".into(),
            "euler_gamma" => "EulerGamma".into(),
            "catalan" => "Catalan".into(),
            "undefined" => "Indeterminate".into(),
            s => ident(s),
        })
    }

    fn list(&self, items: Vec<String>) -> String {
        format!("{{{}}}", items.join(", "))
    }

    fn rel_op(&self, r: Rel) -> &'static str {
        match r {
            Rel::Eq => "==",
            r => r.neutral(),
        }
    }

    fn and_op(&self) -> &'static str {
        "&&"
    }

    fn or_op(&self) -> &'static str {
        "||"
    }

    fn not_op(&self) -> &'static str {
        "!"
    }

    fn prime(&self, p: &Printer, name: &str, order: usize, args: &[Expr]) -> Out {
        Ok(format!(
            "Derivative[{order}][{}][{}]",
            ident(name),
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
                        ident(v)
                    } else {
                        format!("{{{}, {k}}}", ident(v))
                    }
                })
                .collect();
            return Ok(format!("D[{}, {}]", p.print(&f)?, vs.join(", ")));
        }
        let a = p.args(args)?;
        let n = a.len();
        let f = |s: &str| Ok(format!("{s}[{}]", a.join(", ")));
        if self.funcs.iter().any(|f| f.0 == name) || !crate::functions::NEUTRAL.contains(&name) {
            return Ok(format!("{}[{}]", ident(name), a.join(", ")));
        }
        let cap = |s: &str| {
            let mut c = s.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        };
        match (name, n) {
            (
                "exp" | "log" | "sqrt" | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "sinh"
                | "cosh" | "tanh" | "coth" | "sech" | "csch" | "abs" | "sign" | "floor" | "ceiling"
                | "conjugate" | "factorial" | "gamma" | "erf" | "erfc" | "erfi" | "beta"
                | "binomial" | "pochhammer",
                _,
            ) => f(&cap(name)),
            ("zeta", 1) => f("Zeta"),
            // HurwitzZeta is the DLMF's; Zeta[s, a] differs where a < 0
            ("zeta", 2) => f("HurwitzZeta"),
            (
                "asin" | "acos" | "atan" | "acot" | "asec" | "acsc" | "asinh" | "acosh" | "atanh"
                | "acoth" | "asech" | "acsch",
                1,
            ) => f(&format!("Arc{}", cap(&name[1..]))),
            ("root", 2) => Ok(format!("({})^(1/({}))", a[0], a[1])),
            ("atan2", 2) => Ok(format!("ArcTan[{}, {}]", a[1], a[0])),
            ("re", 1) => f("Re"),
            ("im", 1) => f("Im"),
            ("arg", 1) => f("Arg"),
            ("log_gamma", 1) => f("LogGamma"),
            ("digamma", 1) => Ok(format!("PolyGamma[0, {}]", a[0])),
            ("polygamma", 2) => f("PolyGamma"),
            ("fresnel_s", 1) => f("FresnelS"),
            ("fresnel_c", 1) => f("FresnelC"),
            ("ei", 1) => f("ExpIntegralEi"),
            ("li", 1) => f("LogIntegral"),
            ("si", 1) => f("SinIntegral"),
            ("ci", 1) => f("CosIntegral"),
            ("shi", 1) => f("SinhIntegral"),
            ("chi", 1) => f("CoshIntegral"),
            ("expint", 2) => f("ExpIntegralE"),
            ("polylog", 2) => f("PolyLog"),
            ("lerch_phi", 3) => f("LerchPhi"),
            ("lambert_w", 1 | 2) => f("ProductLog"),
            ("bessel_j", 2) => f("BesselJ"),
            ("bessel_y", 2) => f("BesselY"),
            ("bessel_i", 2) => f("BesselI"),
            ("bessel_k", 2) => f("BesselK"),
            ("hankel_1", 2) => f("HankelH1"),
            ("hankel_2", 2) => f("HankelH2"),
            ("airy_ai", 1) => f("AiryAi"),
            ("airy_bi", 1) => f("AiryBi"),
            ("airy_ai_prime", 1) => f("AiryAiPrime"),
            ("airy_bi_prime", 1) => f("AiryBiPrime"),
            // the parameter m, as the formats
            ("elliptic_k", 1) => f("EllipticK"),
            ("elliptic_e", 1 | 2) => f("EllipticE"),
            ("elliptic_f", 2) => f("EllipticF"),
            ("elliptic_pi", 2 | 3) => f("EllipticPi"),
            ("hypergeometric", 3) => f("HypergeometricPFQ"),
            ("legendre_p", 2) => f("LegendreP"),
            ("legendre_q", 2) => f("LegendreQ"),
            ("hermite_h", 2) => f("HermiteH"),
            ("laguerre_l", 2) => f("LaguerreL"),
            ("chebyshev_t", 2) => f("ChebyshevT"),
            ("chebyshev_u", 2) => f("ChebyshevU"),
            ("gegenbauer_c", 3) => f("GegenbauerC"),
            ("jacobi_p", 4) => f("JacobiP"),
            ("heaviside", 1) => f("HeavisideTheta"),
            ("dirac_delta", 1) => f("DiracDelta"),
            ("kronecker_delta", 2) => f("KroneckerDelta"),
            ("struve_h", 2) => f("StruveH"),
            ("struve_l", 2) => f("StruveL"),
            ("kelvin_ber", 2) => f("KelvinBer"),
            ("kelvin_bei", 2) => f("KelvinBei"),
            _ => unsupported(format!("Mathematica: no form for {name} of {n} arguments")),
        }
    }
}

fn build(p: &Problem) -> Result<String, Unsupported> {
    let funcs = p.unknown_functions()?;
    let d = D {
        funcs: funcs.clone(),
    };
    let pr = Printer::new(&d);
    let pe = |e: &Expr| pr.print(e);
    let arg = |k: &str| -> Out { pe(&p.expr(k)?) };
    let items =
        |k: &str| -> Result<Vec<String>, Unsupported> { p.exprs(k)?.iter().map(&pe).collect() };
    let var = |k: &str| -> Out { Ok(ident(&p.symbol(k)?)) };
    let mut assume: Vec<String> = Vec::new();
    for a in p.assumptions()? {
        match a {
            Assumption::Rel(e) => assume.push(pe(&e)?),
            Assumption::Element(s, set) => assume.push(match set.as_str() {
                "naturals" => format!("Element[{0}, Integers] && {0} >= 0", ident(&s)),
                "integers" => format!("Element[{}, Integers]", ident(&s)),
                "rationals" => format!("Element[{}, Rationals]", ident(&s)),
                "reals" => format!("Element[{}, Reals]", ident(&s)),
                _ => format!("Element[{}, Complexes]", ident(&s)),
            }),
        }
    }
    match p.op.as_str() {
        "integrate" | "limit" => assume.push(format!("Element[{}, Reals]", var("var")?)),
        "sum" | "product" => assume.push(format!("Element[{}, Integers]", var("var")?)),
        _ => {}
    }
    let assumptions = if assume.is_empty() {
        "True".to_string()
    } else {
        assume.join(" && ")
    };
    let opt = format!("Assumptions -> {assumptions}");
    let call_of = |(f, vs): &(String, Vec<String>)| {
        format!(
            "{}[{}]",
            ident(f),
            vs.iter().map(|v| ident(v)).collect::<Vec<_>>().join(", ")
        )
    };
    let task = match p.op.as_str() {
        "integrate" => {
            let (f, x) = (arg("integrand")?, var("var")?);
            if p.has("from") || p.has("to") {
                format!(
                    "Integrate[{f}, {{{x}, {}, {}}}, {opt}]",
                    arg("from")?,
                    arg("to")?
                )
            } else {
                format!("Integrate[{f}, {x}, {opt}]")
            }
        }
        "solve_ode" | "solve_pde" => {
            if p.op == "solve_ode" {
                conditions(p, "conditions")?;
            }
            let mut all = if p.op == "solve_ode" {
                items("equations")?
            } else {
                vec![arg("equation")?]
            };
            all.extend(items("conditions")?);
            let f = funcs.first().ok_or(Unsupported("no function".into()))?;
            let fs: Vec<String> = funcs.iter().map(call_of).collect();
            let vars: Vec<String> = f.1.iter().map(|v| ident(v)).collect();
            let vars = if vars.len() == 1 {
                vars[0].clone()
            } else {
                format!("{{{}}}", vars.join(", "))
            };
            format!(
                "DSolve[{{{}}}, {{{}}}, {vars}]",
                all.join(", "),
                fs.join(", ")
            )
        }
        "sum" | "product" => format!(
            "{}[{}, {{{}, {}, {}}}, {opt}]",
            if p.op == "sum" { "Sum" } else { "Product" },
            arg("term")?,
            var("var")?,
            arg("from")?,
            arg("to")?
        ),
        "solve_recurrence" => {
            let mut all = items("equations")?;
            all.extend(items("conditions")?);
            let f = funcs.first().ok_or(Unsupported("no function".into()))?;
            let fs: Vec<String> = funcs.iter().map(call_of).collect();
            format!(
                "RSolve[{{{}}}, {{{}}}, {}]",
                all.join(", "),
                fs.join(", "),
                ident(&f.1[0])
            )
        }
        "limit" => {
            let dir = match p.direction()? {
                Some('+') => ", Direction -> \"FromAbove\"",
                Some(_) => ", Direction -> \"FromBelow\"",
                None => "",
            };
            format!(
                "Limit[{}, {} -> {}{dir}, {opt}]",
                arg("expr")?,
                var("var")?,
                arg("point")?
            )
        }
        "series" => format!(
            "Normal[Series[{}, {{{}, {}, {}}}, {opt}]]",
            arg("expr")?,
            var("var")?,
            arg("point")?,
            p.int("order")? - 1
        ),
        "laplace"
        | "inverse_laplace"
        | "fourier"
        | "inverse_fourier"
        | "mellin"
        | "inverse_mellin"
        | "z_transform"
        | "inverse_z_transform" => {
            p.check_convention()?;
            let (f, x, y) = (arg("expr")?, var("var")?, var("transform_var")?);
            let (name, extra) = match p.op.as_str() {
                "laplace" => ("LaplaceTransform", ""),
                "inverse_laplace" => ("InverseLaplaceTransform", ""),
                // {1, -1}: the formats' exp(-i w t), and 1/(2 pi) in the
                // inverse
                "fourier" => ("FourierTransform", ", FourierParameters -> {1, -1}"),
                "inverse_fourier" => ("InverseFourierTransform", ", FourierParameters -> {1, -1}"),
                "mellin" => ("MellinTransform", ""),
                "inverse_mellin" => ("InverseMellinTransform", ""),
                "z_transform" => ("ZTransform", ""),
                _ => ("InverseZTransform", ""),
            };
            format!("{name}[{f}, {x}, {y}{extra}, {opt}]")
        }
        "solve" => {
            let vars: Vec<String> = p.symbols("vars")?.iter().map(|v| ident(v)).collect();
            let eqs = p.exprs("equations")?;
            let inequality = eqs
                .iter()
                .any(|e| e.is_relation() && !matches!(e, Expr::Rel(Rel::Eq, ..)));
            let domain = if p.real_domain()? || inequality {
                "Reals"
            } else {
                "Complexes"
            };
            let f = if inequality { "Reduce" } else { "Solve" };
            let e = items("equations")?.join(", ");
            if assume.is_empty() {
                format!("{f}[{{{e}}}, {{{}}}, {domain}]", vars.join(", "))
            } else {
                format!(
                    "{f}[{{{e}, {assumptions}}}, {{{}}}, {domain}]",
                    vars.join(", ")
                )
            }
        }
        "simplify" => format!("FullSimplify[{}, {assumptions}]", arg("expr")?),
        "evaluate" | "hyperexpand" => format!("FunctionExpand[{}, {assumptions}]", arg("expr")?),
        "factor" => match poly_domain(p)? {
            PolyDomain::Rationals => format!("Factor[{}]", arg("expr")?),
            PolyDomain::Gaussian => format!("Factor[{}, GaussianIntegers -> True]", arg("expr")?),
        },
        "gcd" => format!("PolynomialGCD[{}]", items("polynomials")?.join(", ")),
        "lcm" => format!("PolynomialLCM[{}]", items("polynomials")?.join(", ")),
        "resultant" => {
            let ps = items("polynomials")?;
            if ps.len() != 2 {
                return unsupported("a resultant of other than two polynomials");
            }
            format!("Resultant[{}, {}, {}]", ps[0], ps[1], var("var")?)
        }
        "discriminant" => format!("Discriminant[{}, {}]", arg("expr")?, var("var")?),
        "partial_fractions" => format!("Apart[{}, {}]", arg("expr")?, var("var")?),
        "groebner" => {
            let order = match monomial_order(p)? {
                MonomialOrder::Lex => "Lexicographic",
                MonomialOrder::Grlex => "DegreeLexicographic",
                MonomialOrder::Grevlex => "DegreeReverseLexicographic",
            };
            let vars: Vec<String> = p.symbols("vars")?.iter().map(|v| ident(v)).collect();
            format!(
                "GroebnerBasis[{{{}}}, {{{}}}, MonomialOrder -> {order}]",
                items("polynomials")?.join(", "),
                vars.join(", ")
            )
        }
        "determinant" | "inverse" | "rank" | "nullspace" | "eigenvalues" | "eigenvectors"
        | "jordan_form" | "solve_linear" => {
            let m = arg("matrix")?;
            match p.op.as_str() {
                "determinant" => format!("Det[{m}]"),
                "inverse" => format!("Inverse[{m}]"),
                "rank" => format!("MatrixRank[{m}]"),
                "nullspace" => format!("NullSpace[{m}]"),
                "eigenvalues" => format!("Eigenvalues[{m}]"),
                // the eigenvalues and their eigenvectors
                "eigenvectors" => format!("Eigensystem[{m}]"),
                // JordanDecomposition gives {S, J}: the form is J
                "jordan_form" => format!("JordanDecomposition[{m}][[2]]"),
                _ => format!("LinearSolve[{m}, {}]", arg("rhs")?),
            }
        }
        op => return unsupported(format!("the op {op} is not known to the runner")),
    };
    Ok(task)
}

fn say(text: &str) -> String {
    format!("WriteString[\"stdout\", \"\\n{text}\\n\"];")
}

impl Adapter for Mathematica {
    fn name(&self) -> &'static str {
        "Mathematica"
    }

    fn slug(&self) -> &'static str {
        "mathematica"
    }

    fn default_command(&self) -> &'static str {
        "wolfram"
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: program.to_string(),
            args: vec!["-noprompt".into()],
            env: vec![],
            dir: None,
        }
    }

    fn start(&self, nonce: &str) -> String {
        // the first use of each solver loads it: done here, untimed
        format!(
            "$HistoryLength = 0; Off[General::stop];\n\
             mrWarm = {{Integrate[x Sin[x], x], Integrate[Exp[-x], {{x, 0, Infinity}}], \
             DSolve[y'[x] == y[x], y[x], x], Limit[Sin[x]/x, x -> 0], Series[Exp[x], {{x, 0, 2}}], \
             Sum[1/n^2, {{n, 1, Infinity}}], Solve[x^2 == 1, x], FullSimplify[Sin[x]^2 + Cos[x]^2], \
             LaplaceTransform[Exp[t], t, s], RSolve[a[n + 1] == 2 a[n], a[n], n]}};\n\
             ClearAll[\"Global`*\"]; ClearSystemCache[];\n\
             WriteString[\"stdout\", \"\\n@@MR-VERSION \", $Version, \"\\n@@MR-READY \", {}, \"\\n\"];\n",
            quoted(nonce)
        )
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        let task = build(p)?;
        let n = quoted(nonce);
        Ok(format!(
            "ClearAll[\"Global`*\"]; ClearSystemCache[];\n\
             {}\n\
             mrC0 = TimeUsed[]; mrW0 = AbsoluteTime[]; mrR = {task}; mrC1 = TimeUsed[]; mrW1 = AbsoluteTime[];\n\
             WriteString[\"stdout\", \"\\n@@MR-TIME \", ToString[NumberForm[mrC1 - mrC0, {{12, 6}}]], \" \", ToString[NumberForm[mrW1 - mrW0, {{12, 6}}]], \"\\n\"];\n\
             WriteString[\"stdout\", \"\\n@@MR-ANSWER \", {n}, \"\\n\", ToString[mrR, InputForm, PageWidth -> Infinity], \"\\n@@MR-ANSWER-END \", {n}, \"\\n\"];\n\
             {}\n",
            say(&format!("@@MR-BEGIN {nonce}")),
            say(&format!("@@MR-END {nonce}"))
        ))
    }

    fn goodbye(&self) -> &'static str {
        "Quit[]"
    }

    fn tested(&self) -> bool {
        false
    }
}
