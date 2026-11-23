//! Maple, one session of its command-line program (`maple -q`) from a pipe,
//! reset with `restart` before each problem. The answer is
//! `lprint(result)`, with `interface(prettyprint = 0, screenwidth =
//! infinity)`: one line that Maple reads back.
//!
//! The assumptions are given with `assuming`, so that no assumed names
//! (`a~`) are left in the answers. Matrices and vectors are written as
//! lists (`convert(M, listlist)`), as the README says.

use super::{Adapter, MonomialOrder, PolyDomain, conditions, monomial_order, poly_domain, quoted};
use crate::expr::{Expr, Nested, Rel};
use crate::print::{Dialect, Out, Printer, Unsupported, diff_args, unsupported};
use crate::problem::{Assumption, Problem};
use crate::session::Spec;

pub struct Maple;

struct D {
    funcs: Vec<(String, Vec<String>)>,
}

/// Names Maple keeps for itself, which a problem's symbol may not be.
const PROTECTED: &[&str] = &["D", "I", "O", "Pi", "gamma", "Digits", "Catalan", "lambda"];

impl Dialect for D {
    fn system(&self) -> &'static str {
        "Maple"
    }

    fn symbol(&self, name: &str) -> Out {
        Ok(match name {
            "pi" => "Pi".into(),
            "e" => "exp(1)".into(),
            "i" => "I".into(),
            "infinity" => "infinity".into(),
            "euler_gamma" => "gamma".into(),
            "catalan" => "Catalan".into(),
            "undefined" => "undefined".into(),
            s if PROTECTED.contains(&s) => {
                return unsupported(format!("the symbol {s} is a name Maple keeps"));
            }
            s => s.to_string(),
        })
    }

    /// `Matrix([[a, b], [c, d]])`, `Vector([a, b])` for a column, `.` the matrix product; `^` is
    /// the matrix power.
    fn matrix(&self, rows: &[Vec<String>]) -> Out {
        let rows: Vec<String> = rows.iter().map(|r| format!("[{}]", r.join(", "))).collect();
        Ok(format!("Matrix([{}])", rows.join(", ")))
    }

    fn vector(&self, items: Vec<String>) -> String {
        format!("Vector([{}])", items.join(", "))
    }

    fn matmul(&self, a: &str, b: &str) -> String {
        format!("{a} . {b}")
    }

    fn rel_op(&self, r: Rel) -> &'static str {
        match r {
            Rel::Ne => "<>",
            r => r.neutral(),
        }
    }

    fn prime(&self, p: &Printer, name: &str, order: usize, args: &[Expr]) -> Out {
        let d = if order == 1 {
            "D".to_string()
        } else {
            format!("(D@@{order})")
        };
        Ok(format!("{d}({name})({})", p.args(args)?.join(", ")))
    }

    /// `int`, `sum`, `product` over `x = a..b`, `limit` at `x = p` (from both sides): the active
    /// forms, which work the form out and keep it as itself where they cannot.
    fn nested(&self, p: &Printer, kind: Nested, body: &Expr, var: &Expr, rest: &[Expr]) -> Out {
        let (f, x) = (p.print(body)?, p.print(var)?);
        Ok(match kind {
            Nested::Limit => format!("limit({f}, {x} = {})", p.print(&rest[0])?),
            kind => {
                let name = match kind {
                    Nested::Integral => "int",
                    Nested::Sum => "sum",
                    _ => "product",
                };
                format!(
                    "{name}({f}, {x} = {}..{})",
                    p.print(&rest[0])?,
                    p.print(&rest[1])?
                )
            }
        })
    }

    fn call(&self, p: &Printer, name: &str, args: &[Expr]) -> Out {
        if name == "subs" {
            let (e, v, a) = crate::print::subs_args(args)?;
            return Ok(format!("eval({}, {v} = {})", p.print(&e)?, p.print(&a)?));
        }
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
                "exp" | "sqrt" | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "sinh" | "cosh"
                | "tanh" | "coth" | "sech" | "csch" | "abs" | "floor" | "conjugate" | "factorial"
                | "erf" | "erfc" | "erfi",
                1,
            ) => f(name),
            (
                "asin" | "acos" | "atan" | "acot" | "asec" | "acsc" | "asinh" | "acosh" | "atanh"
                | "acoth" | "asech" | "acsch",
                1,
            ) => f(&format!("arc{}", &name[1..])),
            ("log", 1) => f("ln"),
            ("log", 2) => Ok(format!("log[{}]({})", a[0], a[1])),
            ("root", 2) => Ok(format!("({})^(1/({}))", a[0], a[1])),
            ("atan2", 2) => f("arctan"),
            ("sign", 1) => f("signum"),
            ("ceiling", 1) => f("ceil"),
            ("re", 1) => f("Re"),
            ("im", 1) => f("Im"),
            ("arg", 1) => f("argument"),
            ("binomial" | "pochhammer", 2) => f(name),
            ("gamma", 1 | 2) => f("GAMMA"),
            ("log_gamma", 1) => f("lnGAMMA"),
            ("digamma", 1) => f("Psi"),
            ("polygamma", 2) => f("Psi"),
            ("beta", 2) => f("Beta"),
            ("fresnel_s", 1) => f("FresnelS"),
            ("fresnel_c", 1) => f("FresnelC"),
            ("ei", 1) => f("Ei"),
            ("li", 1) => f("Li"),
            ("si", 1) => f("Si"),
            ("ci", 1) => f("Ci"),
            ("shi", 1) => f("Shi"),
            ("chi", 1) => f("Chi"),
            // Ei(a, z) is E_a(z)
            ("expint", 2) => f("Ei"),
            ("polylog", 2) => f("polylog"),
            ("zeta", 1) => f("Zeta"),
            ("zeta", 2) => Ok(format!("Zeta(0, {}, {})", a[0], a[1])),
            ("lerch_phi", 3) => f("LerchPhi"),
            ("lambert_w", 1 | 2) => f("LambertW"),
            ("bessel_j", 2) => f("BesselJ"),
            ("bessel_y", 2) => f("BesselY"),
            ("bessel_i", 2) => f("BesselI"),
            ("bessel_k", 2) => f("BesselK"),
            ("hankel_1", 2) => f("HankelH1"),
            ("hankel_2", 2) => f("HankelH2"),
            ("airy_ai", 1) => f("AiryAi"),
            ("airy_bi", 1) => f("AiryBi"),
            ("airy_ai_prime", 1) => Ok(format!("AiryAi(1, {})", a[0])),
            ("airy_bi_prime", 1) => Ok(format!("AiryBi(1, {})", a[0])),
            // Maple's elliptic integrals are of the modulus k, m = k^2, and
            // the incomplete ones of the sine of the amplitude
            ("elliptic_k", 1) => Ok(format!("EllipticK(sqrt({}))", a[0])),
            ("elliptic_e", 1) => Ok(format!("EllipticE(sqrt({}))", a[0])),
            ("elliptic_f", 2) => Ok(format!("EllipticF(sin({}), sqrt({}))", a[0], a[1])),
            ("elliptic_e", 2) => Ok(format!("EllipticE(sin({}), sqrt({}))", a[0], a[1])),
            ("elliptic_pi", 2) => Ok(format!("EllipticPi({}, sqrt({}))", a[0], a[1])),
            ("elliptic_pi", 3) => Ok(format!(
                "EllipticPi(sin({}), {}, sqrt({}))",
                a[1], a[0], a[2]
            )),
            ("hypergeometric", 3) => f("hypergeom"),
            ("legendre_p", 2) => f("LegendreP"),
            ("legendre_q", 2) => f("LegendreQ"),
            ("hermite_h", 2) => f("HermiteH"),
            ("laguerre_l", 2) => f("LaguerreL"),
            ("chebyshev_t", 2) => f("ChebyshevT"),
            ("chebyshev_u", 2) => f("ChebyshevU"),
            ("gegenbauer_c", 3) => f("GegenbauerC"),
            ("jacobi_p", 4) => f("JacobiP"),
            ("heaviside", 1) => f("Heaviside"),
            ("dirac_delta", 1) => f("Dirac"),
            ("kronecker_delta", 2) => Ok(format!("piecewise({} = {}, 1, 0)", a[0], a[1])),
            ("struve_h", 2) => f("StruveH"),
            ("struve_l", 2) => f("StruveL"),
            ("kelvin_ber", 2) => f("KelvinBer"),
            ("kelvin_bei", 2) => f("KelvinBei"),
            _ => unsupported(format!("Maple: no form for {name} of {n} arguments")),
        }
    }
}

struct Script {
    task: String,
    /// The answer, an expression of `mr_r`, the task's value.
    answer: String,
    /// Procedures the task calls, defined after the reset and before the
    /// timing (untimed).
    prelude: &'static str,
}

/// The formats' series, its `order` the power the terms stay below: the terms
/// below `(x - a)^order` (at infinity, below `(1/x)^order`). Maple's `series`
/// takes its order as a truncation of the work, not of the answer: of
/// `exp(-3*x)/x^5` to order 4 it gives the terms up to `x^(-2)`, to order 1 up
/// to `x^0`, and it refuses an order below 0. So `mr_series` expands in
/// `t = x - a` (or `1/x`), raising Maple's order until the series' O term is at
/// the order asked or beyond, and keeps the terms whose power (`t f'/f` of the
/// term, its limit where a logarithm makes it no number) is below it. An order
/// too low to see a denominator's first term is "division by zero series"
/// (`1/(x*atan(x^2))` to order 2): it is raised too.
const SERIES_PROCS: &str = "mr_e := proc(T, t) local e; \
if T = 0 then return infinity end if; \
e := simplify(t*diff(T, t)/T); \
if not type(e, numeric) then e := limit(e, t = 0, right) end if; e end proc:\n\
mr_o := proc(s, t) local z; \
if type(s, series) then order(s) else \
z := indets(s, specfunc(anything, O)); \
if z = {} then infinity else min(op(map(u -> mr_e(op(1, u), t), z))) end if end if end proc:\n\
mr_series := proc(f, x, a, n) local t, g, N, s, o, p, k, T, e, r; \
if a = infinity then g := eval(f, x = 1/t) elif a = -infinity then g := eval(f, x = -1/t) \
else g := eval(f, x = a + t) end if; \
N := max(n, 1); \
for k to 12 do \
try s := series(g, t = 0, N) catch \"division by zero\": if k = 12 then error end if; N := N + 4; next end try; \
o := mr_o(s, t); if o >= n then break end if; N := N + max(1, ceil(n - o)) end do; \
p := convert(s, polynom); r := 0; \
for T in `if`(type(p, `+`), [op(p)], [p]) do e := mr_e(T, t); \
if not (type(e, numeric) and e >= n) then r := r + T end if end do; \
if a = infinity then eval(r, t = 1/x) elif a = -infinity then eval(r, t = -1/x) \
else eval(r, t = x - a) end if end proc:";

fn build(p: &Problem) -> Result<Script, Unsupported> {
    let funcs = p.unknown_functions()?;
    let d = D {
        funcs: funcs.clone(),
    };
    let pr = Printer::new(&d);
    let pe = |e: &Expr| pr.print(e);
    let arg = |k: &str| -> Out { pe(&p.expr(k)?) };
    let items =
        |k: &str| -> Result<Vec<String>, Unsupported> { p.exprs(k)?.iter().map(&pe).collect() };
    let var = |k: &str| -> Out { pr.print(&Expr::Sym(p.symbol(k)?)) };
    // the problem's assumptions, and what the formats assume of a variable
    let mut assume: Vec<String> = Vec::new();
    for a in p.assumptions()? {
        match a {
            Assumption::Rel(e) => assume.push(pe(&e)?),
            Assumption::Element(s, set) => assume.push(match set.as_str() {
                "naturals" => format!("{s}::nonnegint"),
                "integers" => format!("{s}::integer"),
                "rationals" => format!("{s}::rational"),
                "reals" => format!("{s}::real"),
                _ => format!("{s}::complex"),
            }),
        }
    }
    match p.op.as_str() {
        "integrate" | "limit" => assume.push(format!("{}::real", var("var")?)),
        "sum" | "product" => assume.push(format!("{}::integer", var("var")?)),
        _ => {}
    }
    // and the variable of each nested form the same
    for (v, fact) in p.nested_variables()? {
        assume.push(format!("{}::{fact}", pr.print(&Expr::Sym(v))?));
    }
    let mut answer = "mr_r".to_string();
    let mut prelude = "";
    let listlist = |m: &str| format!("convert({m}, listlist)");
    let task = match p.op.as_str() {
        "integrate" => {
            let (f, x) = (arg("integrand")?, var("var")?);
            if p.has("from") || p.has("to") {
                format!("int({f}, {x} = {}..{})", arg("from")?, arg("to")?)
            } else {
                format!("int({f}, {x})")
            }
        }
        "solve_ode" => {
            conditions(p, "conditions")?;
            let mut all = items("equations")?;
            all.extend(items("conditions")?);
            let fs: Vec<String> = funcs
                .iter()
                .map(|(f, vs)| format!("{f}({})", vs.join(", ")))
                .collect();
            if fs.len() == 1 {
                format!("dsolve({{{}}}, {})", all.join(", "), fs[0])
            } else {
                format!("dsolve({{{}}}, {{{}}})", all.join(", "), fs.join(", "))
            }
        }
        "solve_pde" => {
            let f = funcs.first().ok_or(Unsupported("no function".into()))?;
            let call = format!("{}({})", f.0, f.1.join(", "));
            let conds = items("conditions")?;
            if conds.is_empty() {
                format!("pdsolve({}, {call})", arg("equation")?)
            } else {
                format!(
                    "pdsolve([{}, {}], {call})",
                    arg("equation")?,
                    conds.join(", ")
                )
            }
        }
        "sum" | "product" => format!(
            "{}({}, {} = {}..{})",
            p.op,
            arg("term")?,
            var("var")?,
            arg("from")?,
            arg("to")?
        ),
        "solve_recurrence" => {
            let mut all = items("equations")?;
            all.extend(items("conditions")?);
            let fs: Vec<String> = funcs
                .iter()
                .map(|(f, vs)| format!("{f}({})", vs.join(", ")))
                .collect();
            if fs.len() == 1 {
                format!("rsolve({{{}}}, {})", all.join(", "), fs[0])
            } else {
                format!("rsolve({{{}}}, {{{}}})", all.join(", "), fs.join(", "))
            }
        }
        "limit" => {
            let dir = match p.direction()? {
                Some('+') => ", right",
                Some(_) => ", left",
                None => "",
            };
            format!(
                "limit({}, {} = {}{dir})",
                arg("expr")?,
                var("var")?,
                arg("point")?
            )
        }
        "series" => {
            prelude = SERIES_PROCS;
            format!(
                "mr_series({}, {}, {}, {})",
                arg("expr")?,
                var("var")?,
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
            // inttrans's Fourier transform is the formats' one: of
            // exp(-i w t), its inverse with 1/(2 pi)
            let f = match p.op.as_str() {
                "laplace" => "inttrans:-laplace",
                "inverse_laplace" => "inttrans:-invlaplace",
                "fourier" => "inttrans:-fourier",
                "inverse_fourier" => "inttrans:-invfourier",
                "mellin" => "inttrans:-mellin",
                "inverse_mellin" => "inttrans:-invmellin",
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
        "solve" => {
            let vars: Vec<String> = p
                .symbols("vars")?
                .iter()
                .map(|v| pr.print(&Expr::Sym(v.clone())))
                .collect::<Result<_, _>>()?;
            let eqs = p.exprs("equations")?;
            let inequality = eqs
                .iter()
                .any(|e| e.is_relation() && !matches!(e, Expr::Rel(Rel::Eq, ..)));
            // a list of variables: a list of lists of solutions
            let f = if p.real_domain()? {
                "RealDomain:-solve"
            } else {
                "solve"
            };
            let all = if inequality { "" } else { ", allsolutions" };
            format!(
                "{f}({{{}}}, [{}]{all})",
                items("equations")?.join(", "),
                vars.join(", ")
            )
        }
        "simplify" => format!("simplify({})", arg("expr")?),
        "evaluate" => format!("simplify(value({}))", arg("expr")?),
        "hyperexpand" => format!("simplify(convert({}, StandardFunctions))", arg("expr")?),
        "factor" => match poly_domain(p)? {
            PolyDomain::Rationals => format!("factor({})", arg("expr")?),
            PolyDomain::Gaussian => format!("factor({}, I)", arg("expr")?),
        },
        "gcd" | "lcm" => {
            let ps = items("polynomials")?;
            let mut acc = ps
                .first()
                .cloned()
                .ok_or(Unsupported("no polynomials".into()))?;
            for q in &ps[1..] {
                acc = format!("{}({acc}, {q})", p.op);
            }
            acc
        }
        "resultant" => {
            let ps = items("polynomials")?;
            if ps.len() != 2 {
                return unsupported("a resultant of other than two polynomials");
            }
            format!("resultant({}, {}, {})", ps[0], ps[1], var("var")?)
        }
        "minimal_polynomial" => format!(
            "PolynomialTools:-MinimalPolynomial({}, {})",
            arg("expr")?,
            var("var")?
        ),
        "discriminant" => format!("discrim({}, {})", arg("expr")?, var("var")?),
        "partial_fractions" => format!("convert({}, parfrac, {})", arg("expr")?, var("var")?),
        "groebner" => {
            let order = match monomial_order(p)? {
                MonomialOrder::Lex => "plex",
                MonomialOrder::Grlex => "grlex",
                MonomialOrder::Grevlex => "tdeg",
            };
            format!(
                "Groebner:-Basis([{}], {order}({}))",
                items("polynomials")?.join(", "),
                p.symbols("vars")?.join(", ")
            )
        }
        // whether the relation holds for every value: is, true, false or FAIL; its variables
        // real
        "holds" => {
            for v in p.symbols("vars")? {
                assume.push(format!("{}::real", pr.print(&Expr::Sym(v))?));
            }
            format!("is({})", arg("relation")?)
        }
        "matrix_power" => {
            answer = listlist("mr_r");
            format!(
                "LinearAlgebra:-MatrixPower(Matrix({}), {})",
                arg("matrix")?,
                arg("power")?
            )
        }
        "determinant" | "inverse" | "rank" | "nullspace" | "eigenvalues" | "eigenvectors"
        | "jordan_form" | "solve_linear" => {
            let la = "LinearAlgebra:-";
            let m = if p.has("power") {
                format!(
                    "{la}MatrixPower(Matrix({}), {})",
                    arg("matrix")?,
                    arg("power")?
                )
            } else {
                format!("Matrix({})", arg("matrix")?)
            };
            match p.op.as_str() {
                "determinant" if p.has("power") => format!("simplify({la}Determinant({m}))"),
                "determinant" => format!("{la}Determinant({m})"),
                "inverse" => {
                    answer = listlist("mr_r");
                    format!("{la}MatrixInverse({m})")
                }
                "rank" => format!("{la}Rank({m})"),
                "nullspace" => {
                    answer = "map(v -> convert(v, list), [op(mr_r)])".into();
                    format!("{la}NullSpace({m})")
                }
                "eigenvalues" => {
                    answer = "convert(mr_r, list)".into();
                    format!("{la}Eigenvalues({m})")
                }
                "eigenvectors" => {
                    // the eigenvalues, and the eigenvectors as the columns
                    answer = "[convert(mr_r[1], list), convert(mr_r[2], listlist)]".into();
                    format!("[{la}Eigenvectors({m})]")
                }
                "jordan_form" => {
                    answer = listlist("mr_r");
                    format!("{la}JordanForm({m})")
                }
                _ => {
                    answer = "convert(mr_r, list)".into();
                    format!("{la}LinearSolve({m}, Vector({}))", arg("rhs")?)
                }
            }
        }
        op => return unsupported(format!("the op {op} is not known to the runner")),
    };
    let task = if assume.is_empty() {
        task
    } else {
        format!("({task}) assuming {}", assume.join(", "))
    };
    Ok(Script {
        task,
        answer,
        prelude,
    })
}

const SETTINGS: &str =
    "interface(prettyprint = 0, screenwidth = infinity, echo = 0, warnlevel = 0, errorbreak = 0):";

impl Adapter for Maple {
    fn clean_version(&self, text: &str) -> String {
        clean_maple_version(text)
    }

    fn name(&self) -> &'static str {
        "Maple"
    }

    fn slug(&self) -> &'static str {
        "maple"
    }

    /// On Windows the command-line Maple is `cmaple` (`maple` is a Unix
    /// script).
    fn default_command(&self) -> &'static str {
        if cfg!(windows) { "cmaple" } else { "maple" }
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: program.to_string(),
            args: vec!["-q".into()],
            env: vec![],
            dir: None,
        }
    }

    fn start(&self, nonce: &str) -> String {
        format!(
            "{SETTINGS}\nprintf(\"\\n@@MR-VERSION %s\\n@@MR-READY %s\\n\", kernelopts(version), {}):\n",
            quoted(nonce)
        )
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        let s = build(p)?;
        let n = quoted(nonce);
        let prelude = if s.prelude.is_empty() {
            String::new()
        } else {
            format!("{}\n", s.prelude)
        };
        Ok(format!(
            "restart;\n{SETTINGS}\n{prelude}\
             printf(\"\\n@@MR-BEGIN %s\\n\", {n}):\n\
             mr_c0 := time(): mr_w0 := time[real](): mr_ok := false: mr_err := \"\":\n\
             try mr_r := {}; mr_ok := true; catch: mr_err := StringTools:-FormatMessage(lastexception[2..-1]); end try:\n\
             mr_c1 := time(): mr_w1 := time[real]():\n\
             printf(\"\\n@@MR-TIME %.6f %.6f\\n\", mr_c1 - mr_c0, mr_w1 - mr_w0):\n\
             if mr_ok then printf(\"\\n@@MR-ANSWER %s\\n\", {n}); lprint({}); printf(\"@@MR-ANSWER-END %s\\n\", {n}); \
             else printf(\"\\n@@MR-ERROR %s\\n\", mr_err); end if:\n\
             printf(\"\\n@@MR-END %s\\n\", {n}):\n",
            s.task, s.answer
        ))
    }

    fn goodbye(&self) -> &'static str {
        "quit;"
    }

    fn clean_answer(&self, text: &str) -> String {
        // lprint ends its line; a long one is broken with a backslash
        text.trim().replace("\\\n", "")
    }

    fn tested(&self) -> bool {
        false
    }
}

/// `kernelopts(version)` says "Maple 2020.2, APPLE UNIVERSAL OSX, Nov 11 2020,
/// Build ID 1502365": the version is "2020.2"
fn clean_maple_version(text: &str) -> String {
    let first = text
        .split(',')
        .next()
        .unwrap_or(text)
        .trim()
        .trim_matches('"');
    first
        .strip_prefix("Maple")
        .unwrap_or(first)
        .trim()
        .to_string()
}

#[cfg(test)]
mod series_tests {
    use super::*;
    use serde_json::json;

    /// A series to an order of the answer, of any sign: `mr_series`, defined
    /// before the problem's time begins.
    #[test]
    fn a_series_is_cut_at_the_order_asked() {
        let p = Problem::from_json(&json!({
            "id": "s-1",
            "category": "series",
            "problem": {"op": "series", "args": {"expr": "exp(-3*x)/x^5", "var": "x", "point": "0", "order": "-1"}}
        }))
        .unwrap();
        let text = Maple.problem(&p, "n1").unwrap();
        assert!(
            text.contains("try mr_r := mr_series(exp(-3*x)/x^5, x, 0, -1);"),
            "{text}"
        );
        let defined = text.find("mr_series := proc").unwrap();
        assert!(defined < text.find("@@MR-BEGIN").unwrap(), "{text}");
        // other problems are given without it
        let q = Problem::from_json(&json!({
            "id": "s-2",
            "category": "limit",
            "problem": {"op": "limit", "args": {"expr": "sin(x)/x", "var": "x", "point": "0"}}
        }))
        .unwrap();
        assert!(!Maple.problem(&q, "n2").unwrap().contains("mr_series"));
    }
}

#[cfg(test)]
mod version_tests {
    use super::clean_maple_version;

    #[test]
    fn the_version_alone() {
        assert_eq!(
            clean_maple_version("Maple 2020.2, APPLE UNIVERSAL OSX, Nov 11 2020, Build ID 1502365"),
            "2020.2"
        );
        assert_eq!(clean_maple_version("  Maple 2024.1  "), "2024.1");
        assert_eq!(clean_maple_version("2023.0"), "2023.0");
    }
}

#[cfg(test)]
mod nested_tests {
    use super::*;
    use crate::adapters::roundtrip;
    use crate::expr::parse;

    fn print(text: &str) -> String {
        let d = D { funcs: vec![] };
        Printer::new(&d).print(&parse(text).unwrap()).unwrap()
    }

    /// The forms as the system's own, by golden strings.
    #[test]
    fn nested_forms_are_printed() {
        let golden: &[(&str, &str)] = &[
            ("integrate(x^2, x, 0, 3)", "int(x^2, x = 0..3)"),
            ("sum(k, k, 1, 4)", "sum(k, k = 1..4)"),
            ("product(k, k, 1, 4)", "product(k, k = 1..4)"),
            ("limit(sin(x)/x, x, 0)", "limit(sin(x)/x, x = 0)"),
            (
                "sum(integrate(x^3, x, 0, k), k, 1, 3)",
                "sum(int(x^3, x = 0..k), k = 1..3)",
            ),
            (
                "limit((1 + 1/n)^n, n, infinity)",
                "limit((1 + 1/n)^n, n = infinity)",
            ),
            (
                "integrate(exp(-x^2), x, -infinity, infinity)",
                "int(exp(-x^2), x = -infinity..infinity)",
            ),
        ];
        for (neutral, want) in golden {
            assert_eq!(&print(neutral), want, "{neutral}");
        }
    }

    // maple is not installed where this is built: the printing is tested by its golden strings only
}

#[cfg(test)]
mod matrix_tests {
    use super::*;
    use crate::adapters::roundtrip;
    use crate::expr::parse;

    fn print(text: &str) -> String {
        let d = D { funcs: vec![] };
        Printer::new(&d).print(&parse(text).unwrap()).unwrap()
    }

    /// A list of lists is the system's matrix, the product of matrices and of a matrix and a list
    /// the system's product, the power its matrix power.
    #[test]
    fn matrices_are_printed() {
        let golden: &[(&str, &str)] = &[
            (
                "[[1, 2], [3, 4]] * [[0, 1], [1, 0]]",
                "Matrix([[1, 2], [3, 4]]) . Matrix([[0, 1], [1, 0]])",
            ),
            (
                "[[1, 2], [3, 4]] * [1, 1]",
                "Matrix([[1, 2], [3, 4]]) . Vector([1, 1])",
            ),
            ("[[1, 2], [3, 4]]^(-1)", "Matrix([[1, 2], [3, 4]])^(-1)"),
            (
                "2*[[1, 2], [3, 4]] - [[0, 1], [1, 0]]",
                "2*Matrix([[1, 2], [3, 4]]) - Matrix([[0, 1], [1, 0]])",
            ),
        ];
        for (neutral, want) in golden {
            assert_eq!(&print(neutral), want, "{neutral}");
        }
    }
}
