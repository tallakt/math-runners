# Julia adapter: status (work in progress)

Branch `julia` in `~/mr-julia`. This note is for the next session and for the osp.no session. Do not
deploy anything to the site from it yet.

## Where it stands

- `src/adapters/julia.rs`: `--system julia`, default command `julia`. It translates the neutral
  expressions into Symbolics code that a driver runs (`julia_driver.jl`, a persistent session, a
  fresh module per problem, `clock()` for CPU time and `time_ns()` for wall time). Before the
  ready line, the driver warms up on one small problem per op, using the adapter's own code.
- `src/adapters/julia_read.rs`: Julia's answers read back into the neutral syntax. The driver
  prints Symbolics' expression tree as JSON. Rust maps each Julia function, by module and name, to
  the neutral function that is the same function. Any other function makes the answer
  unreadable, an `error` with the reason; nothing is guessed. Symbolics' decline value, `nothing`,
  is written as `nothing`.
- `Adapter::read_answer`, a fallible version of `clean_answer`, is added in `mod.rs` and used by
  `worker.rs`. `main.rs` routes `translate` to `julia::show` and has `julia` in the help text.
- `cargo test`: 35 pass, including the Julia translation tests in both directions.
- **Not yet run against Julia.** The test environment is `~/math-runs/julia-test/env` (use
  `JULIA_PROJECT` to select it). It has Symbolics 7.49.0. The second `Pkg.add` (SpecialFunctions,
  Nemo, Groebner, SymbolicIntegration 3.8.1) was resolved, then stopped during precompile. Run it
  again with `nice -n 19 taskpolicy -b`.

## Seen in Julia 1.12.6 with Symbolics 7.49.0

These results come from short scripts. Package source was not read.

- `sqrt(Num(2))`, `exp(Num(1))`, `sin(Num(π)/6)` and `Num(π)^2` stay symbolic.
  `Num(1)/Num(2)` gives `1//2`.
- `Num(2)^(1//3)` becomes the float 1.2599, and `Num(-8)^(1//3)` throws a DomainError. So
  `2^(1/3)` is sent as `SymbolicUtils.term(^, 2, 1//3)` (`Main.mr_pow`).
- `Num(2)^70` wraps to 0. Rational arithmetic is therefore done in BigInt (`Main.mr_q`).
- In a tree, a number such as π or 2 is a Const: test `isconst` before `iscall`, then use
  `unwrap_const`. The driver does this. Bind `SymbolicUtils = Symbolics.SymbolicUtils`, because it
  is not a direct dependency.
- Install pulls in HypergeometricFunctions, which turns on Symbolics' hypergeometric extension.
  Check what that registers before keeping hyperexpand unsupported.

## Next

1. Finish the install. Start a persistent experiment session (the script is in the scratchpad,
   `jl/server.jl`). Check the following:
   - Registered functions: run `math-run functions -s julia`.
   - SymbolicIntegration's result when it fails.
   - `symbolic_solve_ode` with a plain dependent variable, and `laplace_solve_ode` with
     `@syms y(x)::Real`.
   - `limit`, `taylor` (do the coefficients stay exact?), `laplace` and `inverse_laplace`.
   - `symbolic_solve` periodic roots and `roots_of`.
   - The names of `slog`, `ssqrt` and the C₁ constants.
2. Fill in `polynomial()` and `linear_algebra()`, which are stubs that return unsupported. Look
   at the docstrings for factor, gcd, `groebner_basis` orderings, det, inv, rank and nullspace on
   `Matrix{Num}`, and `symbolic_linear_solve`.
3. Add any function names that Julia's answers actually use to the reader table.
4. Run `math-run version -s julia`. Then run the quick set with `--jobs 2` at low priority,
   writing to `~/math-runs/julia-test/julia-quick.jsonl`. Fix the adapter's faults.
5. Write the README: the adapters row, install steps, the reset/clock row and the Julia section.
   Then finish this note for the site.

## Ops

Integrate (indefinite only) uses SymbolicIntegration.jl (JuliaSymbolics; Risch and rules, no
numeric fitting). This choice needs your decision: without it, integrate is unsupported.

These are unsupported:

- definite integrals
- PDEs, sums, products, recurrences
- Fourier, Mellin and z-transforms
- inequalities, holds, hyperexpand
- ODE systems
- ODE conditions other than the full set of initial values at 0
- series at infinity, or with order ≤ 0

Real-domain `solve` is given to `symbolic_solve`, which works over the complex numbers.
Symbolics has no assumptions, so the problem's assumptions are left out.

Numeric-fitting packages (SymbolicNumericIntegration.jl and similar) are left out entirely, on the
user's decision: numeric fitting is not solving. The SymPy bridges `sympy_*` are also left out,
because they would run SymPy rather than Julia.

## For the site (draft, not final)

The §5.5 line will be: Julia | `Julia` | the neutral syntax of section 3, which the runner writes
from Symbolics' expression tree; `nothing` is Symbolics' decline | `exp(x^2)`.

The checker reads Julia's answers with its neutral parser and grades `nothing` as unsolved. The
version reads `Julia 1.12.6, Symbolics 7.49.0, SymbolicIntegration 3.8.1`.
