# Julia: what the site needs (for the osp.no session)

Branch `julia` of math-runners (`~/mr-julia`). Not merged into main and not pushed. The site
change is deployed only on the user's word.

## The runner

- `math-run run -s julia`, `system.name` `Julia`. The version reads like
  `1.12.6, Symbolics 7.49.0, SymbolicIntegration 3.8.1`, so `math-run version -s julia` prints
  `Julia 1.12.6, Symbolics 7.49.0, SymbolicIntegration 3.8.1`.
- Packages are pinned in `julia/Project.toml` and `julia/Manifest.toml`: Symbolics 7.49.0,
  SymbolicUtils 4.56.1, SymbolicIntegration 3.8.1, Nemo 0.56.2, Groebner 0.10.11,
  SpecialFunctions 2.9.2. They are installed by `scripts/julia-setup.sh` as Julia's shared
  environment `math-run`.
- The user approved SymbolicIntegration.jl (Risch and rules) for indefinite integrals. Numeric
  fitting (SymbolicNumericIntegration.jl and the like) is left out entirely, on the user's
  decision: a fit is not a solution. So are the `sympy_*` bridges, which would run SymPy.
- Nemo, which Symbolics' solvers need, also answers some ops Symbolics lacks: factor, gcd, lcm,
  resultant, discriminant, the minimal polynomial (QQBar), and rank and null space of rational
  matrices.

## §5.5 line (draft)

| system | `system.name` | the answer as | example |
|---|---|---|---|
| Julia | `Julia` | the neutral syntax of section 3, which the runner writes from Symbolics' expression tree, each Julia function by the neutral function that is the same; `nothing` is Symbolics' decline | `exp(x^2)` |

What the checker needs:

- **Read the answers with the neutral parser**, the one it uses for the suite's own answers. No
  new dialect is needed.
- **Grade `nothing` as unsolved.** It is the value by which Symbolics' solvers (`symbolic_solve`,
  `inverse_laplace`, ...) say they cannot solve a problem, a decline.
- **Grade an unevaluated `integrate(f, x)` as unsolved**, the problem returned as other systems
  return it. It is SymbolicIntegration's `∫(f, x)`.
- **Expect Julia's own floats** in some answers, written as decimals:
  - `0.8862269254527579*erf(x)` from the integrator;
  - `6.283185307179586*k` for a periodic family of solutions;
  - `infinity` coefficients in a series at a pole.
- **Expect wrong rationals from `taylor`.** Its default rationalizes float coefficients, so e at
  x = 1 is written `240630938/88523175`. The checker should grade such answers as they are.
- **Read `[[x = 6.283185307179586*k, element(k, integers)]]`.** symbolic_solve's integer of a
  periodic family is an anonymous variable, which the runner names `k`, bound by
  `element(k, integers)`.

An answer with a Julia function that has no neutral counterpart is written as `error`, with the
function named in the message. Examples are `roots_of` and Symbolics' undocumented `scbrt`, which
is the real cube root.

Some functions are written as the neutral function they are seen to be or documented to be:

- Symbolics' undocumented `ssqrt` and `slog` are the principal sqrt and log (`ssqrt(-3 - 4im)` is
  `1 - 2im`, `slog(-1)` is `πim`).
- FresnelIntegrals' `fresnels` and `fresnelc` are `fresnel_s` and `fresnel_c` (DLMF 7.2;
  `fresnels(1.0)` is 0.43826).
- Elliptic's `F(φ, m)` is `elliptic_f` (parameter m; `F(π/2, 0.5)` is K(0.5)).
- PolyLog's `reli(n, x)` is `re(polylog(n, x))`, per its documentation.
- SymbolicIntegration's `hypergeometric2f1(a, b, c, z)` is `hypergeometric([a, b], [c], z)`.

## The quick set on the Mac

Suite 2026.1 quick (2012 problems), `--jobs 2 --memory-gb 4`, timeout 30 s, at low priority:
32 minutes. The run file is `~/math-runs/julia-test/julia-quick.jsonl`.

After reader fixes made during the run, 35 problems whose answers could not be read were run again.
`julia-quick-merged.jsonl` holds the run with their new lines. Use it for the counts, not for
upload.

Totals: 798 ok, 989 unsupported, 195 error, 30 timeout. There were no crashes, hangs, memory stops
or questions.

| op | ok | unsupported | error | timeout |
|---|---|---|---|---|
| integrate (indefinite) | 191 | 0 | 87 | 25 |
| integrate (definite) | 0 | 200 | 0 | 0 |
| solve_ode | 155 | 18 | 43 | 0 |
| simplify | 198 | 0 | 1 | 0 |
| evaluate | 48 | 90 | 0 | 0 |
| solve | 65 | 11 | 11 | 3 |
| series | 49 | 13 | 13 | 0 |
| inverse_laplace | 34 | 0 | 0 | 2 |
| laplace | 1 | 0 | 0 | 0 |
| limit | 1 | 0 | 40 | 0 |
| resultant | 23 | 0 | 0 | 0 |
| minimal_polynomial | 22 | 7 | 0 | 0 |
| factor | 1 | 0 | 0 | 0 |
| determinant | 4 | 1 | 0 | 0 |
| rank | 5 | 0 | 0 | 0 |
| inverse | 1 | 0 | 0 | 0 |
| solve_pde, sum, product, solve_recurrence, fourier, mellin, z-transforms, hyperexpand, holds, eigenvalues, jordan_form, matrix_power | 0 | all | 0 | 0 |

Where the errors come from. All of these are Julia's own; the adapter's faults found in the run
were fixed:

- **integrate.** 85 are SymbolicIntegration's `DomainError: Cannot raise an integer x to a
  negative power`.
- **limit.** Symbolics' `limit` documents that it takes only log, exp and + - * /, so a sin or a
  non-integer power is `Not implemented`.
- **solve_ode.**
  - 16 are `sympy_integrate`, which symbolic_solve_ode calls for some first-order ODEs. That needs
    SymPy, which is not loaded.
  - 19 are `laplace_solve_ode` on nonlinear ODEs.
- **series.** 10 are `InexactError NaN`, from `taylor` at a pole.
- **unreadable.** 2 answers contain Symbolics' `scbrt`, which is the real cube root, not the
  principal one.

## The rented server

`sh scripts/julia-setup.sh /path/to/math-run` does the following:

1. Installs juliaup and Julia 1.12.6.
2. Copies `julia/` into `~/.julia/environments/math-run`.
3. Instantiates the Manifest and precompiles it. This includes `MathRunWarmup`, which compiles a
   small problem of each op into the package images.
4. Runs `math-run version -s julia` and one problem as a smoke test.

Times on the Mac (Apple silicon, at background priority, other work beside):

- About 25 minutes to download and resolve the packages.
- 24 minutes to precompile them.
- 11 minutes for the warm-up package.

Expect much less at normal priority on many cores. The script prints its own time.

The script's steps were run one by one on the Mac: instantiate, precompile, `version`, run. The
script itself was not run end to end, because it would set the Mac's default Julia channel and
write `~/.julia/environments/math-run`. Its first full run will be on the server.

Starting a session takes 54 s with the warm-up package and 11 minutes without it. A session starts
again after each timeout, so the warm-up package matters.

How `--jobs` maps to the machine:

- Each job is one Julia process with one thread. Groebner's tasks use only Julia's threads, and
  there is one.
- A process takes about 1.2 GB after the start and more as it works. `--memory-gb 4` stops a
  problem above that.
- So: `--jobs` at most the physical cores, and RAM of at least 4 GB per job plus the system.
- The reference runs use `--jobs 1`.
