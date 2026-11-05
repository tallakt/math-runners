//! FriCAS, one session (`fricas -nosman`) from a pipe, reset with `)clear
//! all` before each problem. The answer is `unparse(result::InputForm)`.
//!
//! The interpreter goes on after an error, line by line: the task is one
//! line that sets a flag at its end, and the answer is printed only where
//! the flag is set. Where the result is a record (a linear ODE's
//! particular solution and basis, a two-sided limit's sides, eigenvectors)
//! the adapter writes the answer from its fields, as the README says.

use super::{Adapter, MonomialOrder, PolyDomain, conditions, monomial_order, poly_domain};
use crate::expr::{Expr, Rel};
use crate::print::{Dialect, Out, Printer, Unsupported, diff_args, unsupported};
use crate::problem::Problem;
use crate::session::Spec;

pub struct FriCAS;

struct D {
    funcs: Vec<(String, Vec<String>)>,
}

/// An identifier: FriCAS escapes with `_`.
fn ident(name: &str) -> String {
    name.replace('_', "__")
}

impl Dialect for D {
    fn system(&self) -> &'static str {
        "FriCAS"
    }

    fn symbol(&self, name: &str) -> Out {
        Ok(match name {
            "pi" => "%pi".into(),
            "e" => "%e".into(),
            "i" => "%i".into(),
            "infinity" => "%plusInfinity".into(),
            "euler_gamma" | "catalan" | "undefined" => {
                return unsupported(format!("FriCAS has no constant {name}"));
            }
            s => ident(s),
        })
    }

    fn minus_infinity(&self) -> Option<String> {
        Some("%minusInfinity".into())
    }

    fn rel_op(&self, r: Rel) -> &'static str {
        match r {
            Rel::Ne => "~=",
            r => r.neutral(),
        }
    }

    fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out {
        if name == "subs" {
            let (e, v, a) = crate::print::subs_args(args)?;
            return Ok(format!(
                "eval({}, {} = {})",
                p.print(&e)?,
                ident(&v),
                p.print(&a)?
            ));
        }
        if name == "diff" {
            let (f, vars) = diff_args(args)?;
            let mut s = p.print(&f)?;
            for (v, k) in vars {
                s = if k == 1 {
                    format!("D({s}, {})", ident(&v))
                } else {
                    format!("D({s}, {}, {k})", ident(&v))
                };
            }
            return Ok(s);
        }
        let a = p.args(args)?;
        let n = a.len();
        let f = |s: &str| Ok(format!("{s}({})", a.join(", ")));
        let whole = |e: &Expr| matches!(e.as_int(), Some(k) if k >= 0);
        if self.funcs.iter().any(|f| f.0 == name) || !crate::functions::NEUTRAL.contains(&name) {
            return Ok(format!("{}({})", ident(name), a.join(", ")));
        }
        match (name, n) {
            (
                "exp" | "log" | "sqrt" | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "asin"
                | "acos" | "atan" | "acot" | "asec" | "acsc" | "sinh" | "cosh" | "tanh" | "coth"
                | "sech" | "csch" | "asinh" | "acosh" | "atanh" | "acoth" | "asech" | "acsch"
                | "abs" | "floor" | "ceiling" | "conjugate" | "factorial" | "digamma" | "erf"
                | "erfi" | "li",
                1,
            ) => f(name),
            ("log", 2) => Ok(format!("log({})/log({})", a[1], a[0])),
            ("root", 2) => Ok(format!("({})^(1/({}))", a[0], a[1])),
            ("re", 1) => f("real"),
            ("im", 1) => f("imag"),
            ("binomial" | "polygamma" | "polylog", 2) => f(name),
            ("pochhammer", 2) => Ok(format!("Gamma(({}) + ({}))/Gamma({})", a[0], a[1], a[0])),
            ("gamma", 1 | 2) => f("Gamma"),
            ("beta", 2) => f("Beta"),
            ("erfc", 1) => Ok(format!("(1 - erf({}))", a[0])),
            ("fresnel_s", 1) => f("fresnelS"),
            ("fresnel_c", 1) => f("fresnelC"),
            ("ei", 1) => f("Ei"),
            ("si", 1) => f("Si"),
            ("ci", 1) => f("Ci"),
            ("shi", 1) => f("Shi"),
            ("chi", 1) => f("Chi"),
            ("zeta", 1) => f("riemannZeta"),
            ("lerch_phi", 3) => f("lerchPhi"),
            ("lambert_w", 1) => f("lambertW"),
            ("bessel_j", 2) => f("besselJ"),
            ("bessel_y", 2) => f("besselY"),
            ("bessel_i", 2) => f("besselI"),
            ("bessel_k", 2) => f("besselK"),
            ("hankel_1", 2) => f("hankelH1"),
            ("hankel_2", 2) => f("hankelH2"),
            ("airy_ai", 1) => f("airyAi"),
            ("airy_bi", 1) => f("airyBi"),
            ("airy_ai_prime", 1) => f("airyAiPrime"),
            ("airy_bi_prime", 1) => f("airyBiPrime"),
            // FriCAS's incomplete elliptic integrals are of the sine of the
            // amplitude; its parameter m is the formats'
            ("elliptic_k", 1) => f("ellipticK"),
            ("elliptic_e", 1) => f("ellipticE"),
            ("elliptic_f", 2) => Ok(format!("ellipticF(sin({}), {})", a[0], a[1])),
            ("elliptic_e", 2) => Ok(format!("ellipticE(sin({}), {})", a[0], a[1])),
            ("elliptic_pi", 2) => Ok(format!("ellipticPi(1, {}, {})", a[0], a[1])),
            ("elliptic_pi", 3) => Ok(format!("ellipticPi(sin({}), {}, {})", a[1], a[0], a[2])),
            ("hypergeometric", 3) => f("hypergeometricF"),
            ("legendre_p", 2) if whole(&args[0]) => f("legendreP"),
            ("legendre_p", 2) => Ok(format!("legendreP({}, 0, {})", a[0], a[1])),
            ("legendre_q", 2) => Ok(format!("legendreQ({}, 0, {})", a[0], a[1])),
            ("hermite_h", 2) => f("hermiteH"),
            ("laguerre_l", 2) if whole(&args[0]) => f("laguerreL"),
            ("laguerre_l", 2) => Ok(format!("laguerreL({}, 0, {})", a[0], a[1])),
            ("chebyshev_t", 2) if whole(&args[0]) => f("chebyshevT"),
            ("chebyshev_u", 2) if whole(&args[0]) => f("chebyshevU"),
            ("jacobi_p", 4) => f("jacobiP"),
            ("dirac_delta", 1) => f("diracDelta"),
            ("struve_h", 2) => f("struveH"),
            ("struve_l", 2) => f("struveL"),
            ("kelvin_ber", 2) => f("kelvinBer"),
            ("kelvin_bei", 2) => f("kelvinBei"),
            _ => unsupported(format!(
                "FriCAS has no {name} of {n} arguments (of these arguments)"
            )),
        }
    }
}

fn is_infinite(e: &Expr) -> bool {
    matches!(e, Expr::Sym(s) if s == "infinity")
        || matches!(e, Expr::Neg(a) if a.as_sym() == Some("infinity"))
}

struct Script {
    pre: Vec<String>,
    task: String,
    /// FriCAS expressions of a String, tried in order, the first that works
    /// being the answer: of `mrunR`, the task's value.
    answers: Vec<String>,
}

const EXPR: &str = "Expression(Integer)";
const ODE_RECORD: &str =
    "Record(particular: Expression(Integer), basis: List(Expression(Integer)))";

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
    let var = |k: &str| -> Out { Ok(ident(&p.symbol(k)?)) };
    // FriCAS takes no assumptions: the problem is given without them
    p.assumptions()?;
    let pre: Vec<String> = funcs
        .iter()
        .map(|(f, _)| format!("{} := operator '{}", ident(f), ident(f)))
        .collect();
    let plain = "unparse(mrunR::InputForm)".to_string();
    let mut answers = vec![plain.clone()];
    let task = match p.op.as_str() {
        "integrate" => {
            let (f, x) = (arg("integrand")?, var("var")?);
            if p.has("from") || p.has("to") {
                // a definite integral is a union, tagged, which may be failed
                answers.push(
                    "(if mrunR case fail then \"_\"failed_\"\" else if mrunR case pole then \"_\"potentialPole_\"\" else \"@@none\")".into(),
                );
                format!("integrate({f}, {x} = {}..{})", arg("from")?, arg("to")?)
            } else {
                format!("integrate({f}, {x})")
            }
        }
        "solve_ode" => {
            let eqs = p.exprs("equations")?;
            if eqs.len() != 1 || funcs.len() != 1 || funcs[0].1.len() != 1 {
                return unsupported("FriCAS's solve takes one ODE in one function of one variable");
            }
            let (y, x) = (ident(&funcs[0].0), ident(&funcs[0].1[0]));
            let eq = pe(&eqs[0])?;
            let conds = conditions(p, "conditions")?;
            if conds.is_empty() {
                answers = vec![
                    format!(
                        "(mrunO := mrunR :: {ODE_RECORD}; unparse(equation({y}({x}), mrunO.particular + \
                         reduce(+, [concat(\"C\", string(mrunI))::Symbol::{EXPR} * mrunO.basis.mrunI \
                         for mrunI in 1..#mrunO.basis], 0))::InputForm))"
                    ),
                    // a nonlinear ODE: a first integral F, the solution F = C1
                    format!("unparse(equation(mrunR :: {EXPR}, 'C1::{EXPR})::InputForm)"),
                    plain,
                ];
                format!("solve({eq}, {y}, {x})")
            } else {
                // initial values at one point, of the orders 0, 1, ...
                let first = conds[0].at.first().cloned();
                let at = pe(&first.ok_or(Unsupported("a condition at no point".into()))?)?;
                let mut values = vec![None; conds.len()];
                for c in &conds {
                    if c.at.len() != 1
                        || pe(&c.at[0])? != at
                        || c.order >= conds.len()
                        || values[c.order].is_some()
                    {
                        return unsupported(
                            "FriCAS's solve takes initial values y(a), y'(a), ... at one point",
                        );
                    }
                    values[c.order] = Some(pe(&c.value)?);
                }
                let values: Vec<String> = values.into_iter().map(|v| v.unwrap()).collect();
                // explicit, `y(x) = f`, or of a nonlinear ODE implicit: `G` in `y(x)` whose
                // zeros are the solution (the first integral less its value at the point)
                answers = vec![
                    format!(
                        "(mrunE := mrunR :: {EXPR}; if freeOf?(mrunE, {y}({x})) then \
                         unparse(equation({y}({x}), mrunE)::InputForm) else \
                         unparse(equation(mrunE, 0)::InputForm))"
                    ),
                    plain,
                ];
                format!("solve({eq}, {y}, {x} = {at}, [{}])", values.join(", "))
            }
        }
        "solve_pde" => return unsupported("FriCAS has no PDE solver"),
        "sum" | "product" => {
            let (from, to) = (p.expr("from")?, p.expr("to")?);
            if is_infinite(&from) || is_infinite(&to) {
                return unsupported(format!("FriCAS's {} takes no infinite bounds", p.op));
            }
            format!(
                "{}({}, {} = {}..{})",
                p.op,
                arg("term")?,
                var("var")?,
                pe(&from)?,
                pe(&to)?
            )
        }
        "solve_recurrence" => return unsupported("FriCAS has no recurrence solver"),
        "limit" => {
            let dir = match p.direction()? {
                Some('+') => ", \"right\"",
                Some(_) => ", \"left\"",
                None => "",
            };
            let sides = "Record(leftHandLimit: Union(OrderedCompletion(Expression(Integer)), \"failed\"), \
                         rightHandLimit: Union(OrderedCompletion(Expression(Integer)), \"failed\"))";
            answers = vec![
                plain,
                format!(
                    "(mrunO := mrunR :: {sides}; concat([\"[leftHandLimit = \", \
                     unparse(mrunO.leftHandLimit::InputForm), \", rightHandLimit = \", \
                     unparse(mrunO.rightHandLimit::InputForm), \"]\"]))"
                ),
            ];
            format!(
                "limit(({})::{EXPR}, {} = {}{dir})",
                arg("expr")?,
                var("var")?,
                arg("point")?
            )
        }
        "series" => {
            let point = p.expr("point")?;
            if is_infinite(&point) {
                return unsupported("FriCAS's series is not at infinity");
            }
            format!(
                "approximate(series(({})::{EXPR}, {} = {}), {})",
                arg("expr")?,
                var("var")?,
                pe(&point)?,
                p.int("order")? - 1
            )
        }
        "laplace" | "inverse_laplace" => {
            p.check_convention()?;
            let (f, x, y) = (arg("expr")?, var("var")?, var("transform_var")?);
            if p.op == "laplace" {
                format!("laplace(({f})::{EXPR}, {x}, {y})")
            } else {
                format!("inverseLaplace(({f})::{EXPR}, {x}, {y})")
            }
        }
        "fourier"
        | "inverse_fourier"
        | "mellin"
        | "inverse_mellin"
        | "z_transform"
        | "inverse_z_transform" => {
            return unsupported(format!("FriCAS has no {}", p.op));
        }
        "solve" => {
            let eqs = p.exprs("equations")?;
            if eqs
                .iter()
                .any(|e| e.is_relation() && !matches!(e, Expr::Rel(Rel::Eq, ..)))
            {
                return unsupported("FriCAS does not solve inequalities");
            }
            let vars: Vec<String> = p.symbols("vars")?.iter().map(|v| ident(v)).collect();
            // solve gives the real solutions, radicalSolve the complex ones
            let f = if p.real_domain()? {
                "solve"
            } else {
                "radicalSolve"
            };
            if eqs.len() == 1 && vars.len() == 1 {
                format!("{f}({}, {})", pe(&eqs[0])?, vars[0])
            } else {
                format!("{f}({}, [{}])", list("equations")?, vars.join(", "))
            }
        }
        "simplify" => format!("simplify(({})::{EXPR})", arg("expr")?),
        "evaluate" => format!("({})::{EXPR}", arg("expr")?),
        "hyperexpand" => return unsupported("FriCAS has no hypergeometric expansion"),
        "factor" => match poly_domain(p)? {
            PolyDomain::Rationals => format!("factor({})", arg("expr")?),
            PolyDomain::Gaussian => {
                format!("factor(({})::Polynomial(Complex(Integer)))", arg("expr")?)
            }
        },
        "gcd" | "lcm" => format!("{}({})", p.op, list("polynomials")?),
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
        "minimal_polynomial" => {
            return unsupported(
                "FriCAS: no minimal polynomial of an algebraic expression in the runner",
            );
        }
        "discriminant" => format!("discriminant({}, {})", arg("expr")?, var("var")?),
        "partial_fractions" => {
            answers = vec![
                "concat([unparse(wholePart(mrunR)::InputForm), concat([concat([\" + (\", \
                 unparse(mrunF.num::InputForm), \")/(\", unparse(mrunF.d_fact::InputForm), \")^\", \
                 string(mrunF.d_exp)]) for mrunF in fractionalTerms(mrunR)])])"
                    .into(),
            ];
            format!(
                "partialFraction(({})::Fraction(Polynomial(Integer)), {})",
                arg("expr")?,
                var("var")?
            )
        }
        "groebner" => {
            let vars: Vec<String> = p.symbols("vars")?.iter().map(|v| ident(v)).collect();
            let ring = match monomial_order(p)? {
                MonomialOrder::Lex => "DistributedMultivariatePolynomial",
                MonomialOrder::Grevlex => "HomogeneousDistributedMultivariatePolynomial",
                MonomialOrder::Grlex => {
                    return unsupported("FriCAS: no grlex domain in the runner");
                }
            };
            format!(
                "groebner([mrunP::{ring}([{}], Fraction(Integer)) for mrunP in {}])",
                vars.join(", "),
                list("polynomials")?
            )
        }
        "holds" => return unsupported("FriCAS has no decision whether a relation holds"),
        "matrix_power" => {
            return unsupported("FriCAS has no power of a matrix to a symbolic exponent");
        }
        "determinant" | "inverse" | "rank" | "nullspace" | "eigenvalues" | "eigenvectors"
        | "jordan_form" | "solve_linear" => {
            if p.has("power") {
                return unsupported("FriCAS has no power of a matrix to a symbolic exponent");
            }
            let m = format!("matrix({})", arg("matrix")?);
            match p.op.as_str() {
                "determinant" => format!("determinant({m})"),
                "inverse" => format!("inverse({m})"),
                "rank" => format!("rank({m})"),
                "nullspace" => format!("nullSpace({m})"),
                "eigenvalues" => format!("radicalEigenvalues({m})"),
                "eigenvectors" => {
                    answers = vec![
                        "concat([\"[\", concat([concat([\"[\", unparse(mrunE.radval::InputForm), \", \", \
                         unparse(mrunE.radmult::InputForm), \", \", unparse(mrunE.radvect::InputForm), \"], \"]) \
                         for mrunE in mrunR]), \"]\"])"
                            .into(),
                    ];
                    format!("radicalEigenvectors({m})")
                }
                "solve_linear" => {
                    answers = vec![
                        // the solution vector, particular + t1 basis.1 + ...
                        "unparse(vector([((mrunR.particular :: Vector(Fraction(Integer))).mrunJ)::Expression(Integer) + \
                         reduce(+, [concat(\"t\", string(mrunI))::Symbol::Expression(Integer) * \
                         ((mrunR.basis.mrunI).mrunJ)::Expression(Integer) for mrunI in 1..#mrunR.basis], 0) \
                         for mrunJ in 1..#(mrunR.particular :: Vector(Fraction(Integer)))])::InputForm)"
                            .into(),
                        "concat([\"[particular = \", unparse(mrunR.particular::InputForm), \
                         \", basis = \", unparse(mrunR.basis::InputForm), \"]\"])"
                            .into(),
                    ];
                    format!("solve({m}, vector({}))", arg("rhs")?)
                }
                _ => return unsupported("FriCAS has no Jordan form"),
            }
        }
        op => return unsupported(format!("the op {op} is not known to the runner")),
    };
    Ok(Script { pre, task, answers })
}

impl Adapter for FriCAS {
    fn clean_version(&self, text: &str) -> String {
        // "FriCAS 1.4.0": the version
        let t = text.trim();
        t.strip_prefix("FriCAS ").unwrap_or(t).to_string()
    }

    fn name(&self) -> &'static str {
        "FriCAS"
    }

    fn slug(&self) -> &'static str {
        "fricas"
    }

    fn default_command(&self) -> &'static str {
        "fricas"
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: program.to_string(),
            args: vec!["-nosman".into()],
            env: vec![],
            dir: None,
        }
    }

    fn start(&self, nonce: &str) -> String {
        // the first use of the integrator, the limits and the simplifier
        // loads them: done here, untimed
        format!(
            ")set output algebra off\n)set message type off\n)set message prompt none\n\
             )set message time off\n)set quit unprotected\n\
             integrate(sin(x)^2, x)\nlimit(sin(x)/x, x = 0)\nsimplify(sin(x)^2 + cos(x)^2)\n\
             )clear all\n\
             )lisp (progn (format t \"~%@@MR-VERSION ~a~%@@MR-READY {nonce}~%\" |$build_version|) nil)\n"
        )
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        let s = build(p)?;
        let mut out = String::from(")clear all\n");
        for l in &s.pre {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str(&format!(
            ")lisp (progn (format t \"~%@@MR-BEGIN {nonce}~%\") (setq |mrunC0| (get-internal-run-time) |mrunW0| (get-internal-real-time)) nil)\n\
             (mrunR := {}; mrunOK := true)\n\
             )lisp (progn (format t \"~%@@MR-TIME ~,6f ~,6f~%\" (/ (- (get-internal-run-time) |mrunC0|) internal-time-units-per-second) (/ (- (get-internal-real-time) |mrunW0|) internal-time-units-per-second)) nil)\n\
             mrunT := \"@@none\"\n",
            s.task
        ));
        for a in &s.answers {
            out.push_str(&format!(
                "if mrunOK = true and mrunT = \"@@none\" then mrunT := {a}\n"
            ));
        }
        out.push_str(&format!(
            "if mrunOK = true and mrunT ~= \"@@none\" then (PRINC(\"@@MR-ANSWER {nonce}\")$Lisp; TERPRI()$Lisp; \
             PRINC(mrunT)$Lisp; TERPRI()$Lisp; PRINC(\"@@MR-ANSWER-END {nonce}\")$Lisp; TERPRI()$Lisp)\n\
             )lisp (progn (format t \"~%@@MR-END {nonce}~%\") nil)\n"
        ));
        Ok(out)
    }

    fn goodbye(&self) -> &'static str {
        ")quit"
    }

    fn clean_answer(&self, text: &str) -> String {
        // the lists the adapter writes from records end in ", ]"
        text.trim().replace(", ]", "]")
    }

    fn clean_noise(&self, text: &str) -> String {
        // the first error, without the interpreter's advice
        let mut out = Vec::new();
        for l in text.lines() {
            let t = l.trim();
            if t.starts_with("Value =") || t.is_empty() || t.starts_with("All user variables") {
                continue;
            }
            if t.starts_with("Perhaps you should")
                || t.starts_with("or \"$\"")
                || t.contains("HyperDoc")
            {
                break;
            }
            out.push(t.to_string());
        }
        out.join(" ")
    }
}
