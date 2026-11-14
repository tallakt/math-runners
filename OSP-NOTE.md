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
function named in the message. Examples are `PolyLog.reli`, Symbolics' undocumented
`ssqrt`/`scbrt`/`slog`, and `roots_of`.

## The quick set on the Mac

Run with `--jobs 2` at low priority. The results are in `~/math-runs/julia-test/julia-quick.jsonl`;
the summary is in the final report.

## The rented server

`sh scripts/julia-setup.sh /path/to/math-run` does the following:

1. Installs juliaup and Julia 1.12.6.
2. Copies `julia/` into `~/.julia/environments/math-run`.
3. Instantiates the Manifest and precompiles it. This includes `MathRunWarmup`, which compiles a
   small problem of each op into the package images.
4. Runs `math-run version -s julia` and one problem as a smoke test.

Times on the Mac (Apple silicon, at background priority): 24 minutes to precompile the packages,
11 minutes for the warm-up package. Expect less at normal priority on many cores.

Starting a session takes 54 s with the warm-up package and 11 minutes without it. A session starts
again after each timeout, so the warm-up package matters.

How `--jobs` maps to the machine:

- Each job is one Julia process with one thread. Groebner's tasks use only Julia's threads, and
  there is one.
- A process takes about 1.2 GB after the start and more as it works. `--memory-gb 4` stops a
  problem above that.
- So: `--jobs` at most the physical cores, and RAM of at least 4 GB per job plus the system.
- The reference runs use `--jobs 1`.
