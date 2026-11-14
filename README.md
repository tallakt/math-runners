# math-run

`math-run` gives the suite of [math.osp.no](https://math.osp.no) to a computer algebra system,
times it, and writes the run file that the site takes, and uploads it. The formats are the site's
page [/formats](https://math.osp.no/formats): the suite (section 2), the expressions (3), the ops
(4), the run file (5) and the upload API (6).

One binary has an adapter for each system:

| system | `--system` | program it starts | tested with |
|---|---|---|---|
| SymPy | `sympy` | `python3` (with SymPy installed) | SymPy 1.14.0, Python 3.13, macOS |
| Maxima | `maxima` | `maxima` | Maxima 5.50.0 (SBCL), macOS; Maxima 5.47.0 (GCL, Debian trixie), Linux |
| FriCAS | `fricas` | `fricas` | FriCAS 1.4.0 (SBCL), macOS; FriCAS 1.3.11 (GCL, Debian trixie), Linux |
| Giac | `giac` | `giac` (or `icas`) | Giac 1.9.0.93 (Debian trixie), Linux |
| Julia | `julia` | `julia` (Symbolics.jl and SymbolicIntegration.jl, the versions of `julia/Manifest.toml`) | Julia 1.12.6, Symbolics 7.49.0, SymbolicIntegration 3.8.1, macOS |
| Maple | `maple` | `maple` | **not run**: written from the documentation, see below |
| Mathematica | `mathematica` | `wolfram` (or `math`, `WolframKernel`) | **not run**: written from the documentation |
| Mathilda | `mathilda` | `Mathilda` | **not run**: the Mathematica adapter's problems, from Mathilda's user documentation |

## Install

`math-run` runs on Linux (x86_64, aarch64) and macOS (arm64, and Intel by building it); not on
Windows (use WSL). It needs the network only to fetch the suite and to upload.

Download the binary for your machine from the site's [downloads](https://math.osp.no/downloads)
(check the SHA-256 given there), unpack it (`tar xzf math-run-0.1.0-linux-x86_64.tar.gz`; GNU tar
may warn about `LIBARCHIVE.xattr` keywords: harmless), and run `math-run-.../math-run`, or put it
on the `PATH`. Or build it, with Rust 1.85 or later (`rustup`, or your package manager's `cargo`;
the build fetches its crates from crates.io):

```
cargo build --release        # target/release/math-run
```

Install the system you run, so that its program is on the `PATH`, or give the program with
`--command`:

- **SymPy**: Debian, Ubuntu: `apt install python3-sympy` (the plain `python3` then has it; a fresh
  Debian has neither `pip` nor `venv`). Elsewhere: `python3 -m venv venv && venv/bin/pip install
  sympy`, then `--command venv/bin/python`. Without SymPy, `math-run` says so and stops.
- **Maxima**: your package manager's `maxima` (Homebrew: `brew install maxima`).
- **FriCAS**: `fricas` (Homebrew: `brew install fricas`; Debian: `apt install fricas`).
- **Giac**: the command-line `giac` of the `xcas` package (Debian, Ubuntu), or Bernard Parisse's
  builds.
- **Julia**: `sh scripts/julia-setup.sh [path/to/math-run]` installs juliaup and Julia 1.12.6, and
  the packages of `julia/Project.toml` and `julia/Manifest.toml` (Symbolics, SymbolicIntegration,
  SpecialFunctions, Nemo, Groebner, at exactly those versions, and the warm-up package
  `julia/MathRunWarmup`, see the Julia section) as Julia's shared environment `math-run`,
  instantiated and precompiled, and then gives Julia one problem. By hand: `curl -fsSL
  https://install.julialang.org | sh`, `juliaup add 1.12.6`, copy `julia/Project.toml`,
  `julia/Manifest.toml` and `julia/MathRunWarmup/` into `~/.julia/environments/math-run/`, and
  `julia --project=@math-run -e 'using Pkg; Pkg.instantiate(); Pkg.precompile()'`. The precompile
  takes long: on the Mac (Apple silicon, at low priority) 24 minutes for the packages and 11 for
  the warm-up. The runner starts `julia --project=@math-run`; set `JULIA_PROJECT` to use another
  environment. Each `--jobs` is a Julia process of its own, one thread, about 1.2 GB after the
  start, more as it works: give each job a physical core and 4 GB (`--memory-gb 4`).
- **Maple**: the `maple` command-line script of your Maple installation, as
  `/Applications/Maple 2023/maple` on macOS.
- **Mathematica**: the command-line kernel, `wolfram` (Wolfram Engine, Mathematica).

`math-run version -s <system>` starts the system and prints its version, as the run file will
have it. Do this first: a system that is not installed, or a `--command` that is not the system's
command-line program, is told here and not after the suite is fetched (a program that prints
nothing and computes nothing for 20 seconds is taken as not being the system).

## Run

```
math-run run -s maxima                          # the newest suite, every problem
math-run run -s maxima --subset quick           # the quick subset, about 2,000
math-run run -s sympy --command venv/bin/python --timeout 30 -o sympy-full.jsonl
```

`run` fetches the suite (`GET /api/suites`), checks its SHA-256 against the site's, and keeps it in
a cache (`~/.cache/math-run`, or `~/Library/Caches/math-run` on macOS, or `$MATH_RUN_CACHE`). It
starts the system, reads its version from it, gives it the calibration problems and then the rest
of the subset, one at a time, and writes each answer to the run file as it comes. At the end it
writes the time it finished into the header.

The options:

| option | | default |
|---|---|---|
| `--system`, `-s` | the system | |
| `--command` | the program that starts it | as in the table above |
| `--subset` | `full` or `quick` | `full` |
| `--timeout` | the seconds each problem has | 30 |
| `--jobs` | problems at a time, each in a session of its own | 1 |
| `--restart-every` | start the system again after this many problems, 0 never | 500 |
| `--memory-gb` | stop a problem whose system (all its processes) holds more than this many GB, as an `error` (checked four times a second) | no limit |
| `--out`, `-o` | the run file | `<system>-<subset>-<time>.jsonl` |
| `--suite`, `--suite-version` | a local suite file instead of the site's, and its version | |
| `--site` | the site | `https://math.osp.no` |
| `--only`, `--limit` | some problems only, for tests | |
| `--verbose`, `-v` | print what the system prints | |
| `--note` | free text on how the run was made, at most 2000 bytes, for the header's `note`: the run's page shows it | |

**Timing.** The reference runs of the site are made on a bare-metal server, one problem at a
time. Keep `--jobs 1` for runs whose times are to be compared: problems run side by side share the
machine's memory and caches. With more jobs the header says how many (`"jobs"`).

The 30-second timeout is a suggestion: a run of the whole suite, about 60,000 problems, then
takes a day or less, the most of it the problems that time out.

### What a problem gets

- **A fresh state.** The system is started once and kept, as starting it for each problem would
  cost more than most problems take; before each problem its state is reset (the table below).
  The system is started again after a problem that runs out of time, fails it, or asks for input,
  and every `--restart-every` problems.
- **The timeout.** The runner kills the system (its whole process group) when a problem runs out
  of time, and starts it again for the next.
- **The times.** `wall` and `cpu` are of the system's work on the problem only: the start of the
  system and the reset are not in them, nor printing the answer. Where the system has a clock, the
  times are taken in it, around the command (the table below); Giac has none, and the runner takes
  them itself from the line Giac prints when its work is done, the CPU time from the operating
  system. For a problem that runs out of time, `wall` is the time it had and `cpu` the CPU time the
  operating system counted.
- **A crash.** A system that ends (its exit status, or the signal that killed it, is in the
  message; signal 9 is usually the kernel's out-of-memory killer) is `error` for that problem and
  is started again for the next. Set `--memory-gb` to stop a problem before the machine swaps.
  If `math-run` itself is killed (`kill -9`), Linux kills the system with it; on macOS a system
  that was computing keeps going until it is done: look for it in `ps` before you resume.
- **Questions.** A system that stops to ask (as Maxima's "Is a positive, negative or zero?") is
  seen as waiting: no output and no CPU time for 3 seconds. The problem is written as `error`, with
  the question as its message.

| system | reset | clock |
|---|---|---|
| SymPy | a new namespace, `clear_cache()` | `time.process_time()`, `time.perf_counter()` |
| Maxima | `kill(all)`, `reset()` | `elapsed_run_time()`, `elapsed_real_time()` |
| FriCAS | `)clear all` | the Lisp's `get-internal-run-time`, `get-internal-real-time` |
| Giac | `restart` | none: the runner's, see above |
| Julia | a new module | C's `clock()`, `time_ns()` |
| Maple | `restart` | `time()`, `time[real]()` |
| Mathematica | `ClearAll["Global`*"]`, `ClearSystemCache[]` | `TimeUsed[]`, `AbsoluteTime[]` |

Each system first loads the parts of itself that it loads at first use (the integrator, the ODE
solver, ...), before the first problem, so that the first problem of each kind is not timed with
the loading. Maxima's code that is written in Maxima (as `simplify_sum`) is unloaded by `kill(all)`:
it is loaded again before each problem that needs it, untimed. Maple's `restart` clears its memory
of the library too: a problem's time in Maple includes reading the library's procedures it uses.

### The status of each problem

- `ok`: the system returned. The answer is what it returned, in its own input syntax, as the
  table below says; also where it returned the problem unevaluated, or declined it (the site's
  checker grades both as unsolved, section 7.4 of the formats). A decline is the system's own way
  of saying that it cannot do the problem, written as an answer:
  - **SymPy**: a `NotImplementedError` (or a subclass of it), written `NotImplementedError("its
    message")`, a subclass's name before the message; any other exception is `error`.
  - **Maxima**: the problem returned as a noun (`'integrate(...)`), or `false` (as from `ode2`),
    written as they are.
  - **FriCAS**: `"failed"` or `"potentialPole"` of a definite integral, written so; an integral it
    cannot do otherwise comes back as `integral(...)`.
  - **Giac**: the problem returned unevaluated (`integrate(...)`, `desolve(...)`); what Giac
    prints as an error stays `error`.
  - **Julia**: `nothing`, the value by which Symbolics' solvers (`symbolic_solve`,
    `inverse_laplace`, ...) say they cannot solve the problem, written `nothing`.
  - **Maple**: `FAIL`, and the unevaluated `int(...)`, `DESol(...)`.
  - **Mathematica**: `$Failed`, and the unevaluated `Integrate[...]`, `DSolve[...]`.
- `timeout`: it ran out of time.
- `error`: it failed (its message), asked for input, or printed an answer over 1 MB.
- `unsupported`: the runner has no way to give the system this problem: the system has no command
  for the op (as Maxima for Fourier transforms), or for one of the problem's functions (as SymPy
  for the Struve functions), or the runner cannot read the problem. The message says which.

## Resume

A run that stops (Ctrl-C, a reboot) resumes where it stopped:

```
math-run resume maxima-full-2026-11-04T100000Z.jsonl
```

The run's options are kept beside the run file, in `<file>.state.json` (it is written when the run
begins; keep it with the run file). The run resumes only with the same suite, subset, timeout and
system version; it warns where the machine is another. A line cut short by the interruption (also
by `kill -9` or a power cut) is removed, and the problems already answered are not given again, so
no problem is answered twice or lost. The header counts the resumes (`"resumed"`). The run file
is written as the problems come, one line each; do not resume it from two terminals at once.

## Upload

Register at the site's `/register` with your email address; the link we mail you shows your upload
token, and `/account` (log in) shows it again, or makes a new one, which replaces the old. Give
the token as the environment variable `MATH_OSP_TOKEN` (it is not kept in any file, and not in the
shell's history or the process list as `--token` would be):

```
export MATH_OSP_TOKEN=...
math-run upload maxima-full-2026-11-04T100000Z.jsonl
math-run status r-k3j9x2m4q8
```

`upload` gzips the file and posts it to `POST /api/runs` (`--site` for another site, as a local
copy of it: `--site http://math.localhost:4000`). The site checks the whole file before it answers,
and then grades it in the background; `status` tells how far it is, and the totals once graded.
`upload --note "..."` sends the run with that note in its header (an empty one removes it), and
leaves the file as it is; the uploader can change the note later on the run's page.

- **A run that did not finish** (interrupted, not resumed) is not uploaded: `resume` it, or add
  `--unfinished` to upload what there is; the problems not reached count as missing.
- **The same file twice.** After the site takes a run, `<file>.uploaded.json` records its run id;
  `upload` of that file again refuses, as it would make a second run on the site. `--again`
  uploads it anyway.
- **Errors** say what happened: no token, or one the site does not take (401: see `/account`); a
  file the site refuses (422: its faults are listed, and nothing is kept); no network, or a site
  that does not answer in 10 minutes (nothing, or maybe the file, reached the site: the message
  says which; look at your runs on the site before uploading again if it says maybe). The exit
  status is 1 for each.

## The run file

The header (section 5.1 of the formats) has the machine as `math-run machine` prints it: the CPU's
model, the cores (physical; `threads` as well where there are more), the memory in GB, and the
operating system. Besides the formats' fields it has `jobs`, and `resumed` where the run was
resumed.

The machine's `id` is a salted hash, the first 16 hex digits of the SHA-256 of `math.osp.no machine `
and the operating system's own id of the machine (macOS's IOPlatformUUID, Linux's
`/etc/machine-id`): the raw id does not leave the machine. The site uses it only to tell that two
runs were made on the same machine, as for the speed estimate of a system whose runs are all on
one machine. Set `MATH_RUN_NO_MACHINE_ID=1` to leave it out; the site then matches the runs by
their CPU, cores, threads and memory.

Each problem's line is section 5.2's: `id`, `status`, `answer` (with `ok`), `wall` and `cpu` (in
seconds), and `message` (with `error` and `unsupported`).

**What is private.** The upload holds the header above and the lines of the problems, nothing else:
not your user name, host name, paths, the program given to `--command`, or the token (which is
the request's header, not the file). A system's error message can carry them (a Python
traceback has file paths), so the messages are scrubbed as they are written: your home directory
becomes `~`, the working directory `.`, and your user name and host name `<user>` and `<host>`.
`<file>.state.json` (with the paths) stays on your machine. Read the file before uploading if in
doubt: it is plain JSON lines.

## What each adapter gives the system, and writes

The expressions of the suite are translated into the system's syntax: its names for the functions
(`bessel_j(nu, x)` is `besselj` in SymPy, `bessel_j` in Maxima, `besselJ` in FriCAS, `BesselJ` in
Giac, Maple and Mathematica), its constants (`%pi`, `Pi`, `E`, ...) and its operators. A function
the system lacks is written another way where that is the same function (`log(b, x)` as
`log(x)/log(b)`; in FriCAS `erfc(x)` as `1 - erf(x)` and `pochhammer(a, n)` as
`Gamma(a + n)/Gamma(a)`; in Giac the Hankel functions as `BesselJ + i BesselY`); else the problem
is `unsupported`. `math-run functions -s <system>` gives the system each function of the formats'
table.

Where the formats name no convention, the runner chose (and the checker should know):

- **A derivative in a condition**, `subs(diff(y(x), x), x = 0) = 2` in the formats, is given as
  each system takes it: SymPy's `ics` (`y(x).diff(x).subs(x, 0)`), Maxima's `ic2` or `atvalue`,
  FriCAS's initial values, Giac's `y'(0)`, Maple's `eval(diff(y(x), x), x = 0)` (which is
  `D(y)(0)`), Mathematica's `D[y[x], x] /. x -> 0` (which is `y'[0]`). The runner also reads
  `y'(0) = 2`, primes after the function's name.
- **The variable** of integration and of a limit is given as real, of a sum or a product as an
  integer, where the system takes such an assumption (SymPy's symbols, Maple's `assuming`,
  Mathematica's `Assumptions`). Maxima and Giac take their symbols as real anyway; FriCAS takes no
  assumptions.
- **The problem's assumptions** (`a > 0`, `element(n, integers)`) are given as the system takes
  them: SymPy's symbol assumptions (`positive=True`, `integer=True`, other relations by `refine` of
  a simplification), Maxima's `assume` and `declare`, Giac's `assume`, Maple's `assuming`,
  Mathematica's `Assumptions`. FriCAS takes none: its problems are given without them.
- **`domain`** of `factor`: `rationals` or `integers` (the default), or `gaussian` for the Gaussian
  rationals; another is `unsupported`. **`order`** of `groebner`: `lex`, `grlex` or `grevlex`.
- **`convention`** of a transform: only the formats' default; another is `unsupported`.
- **`order`** of a series: the systems that truncate at a power (SymPy's and Maple's `series`)
  get `order`; those that keep the powers up to one (Maxima's `taylor`, FriCAS's `approximate`,
  Giac's and Mathematica's `Series`) get `order - 1`. The two agree for whole powers, of
  `var - point` or, at infinity, of `1/var`; a Puiseux series' fractional powers between
  `order - 1` and `order` are left out by the second kind.

The commands, and the part of the result that is written where it is not the whole:

### SymPy

The runner writes Python code for SymPy, which a small driver (`src/adapters/sympy_driver.py`) runs
in a fresh namespace; the answer is `str(result)` (a matrix printed on one line), and a
`NotImplementedError` the decline `NotImplementedError("...")` (above).

| op | command | written |
|---|---|---|
| integrate | `integrate(f, x)`, `integrate(f, (x, a, b))` | |
| solve_ode | `dsolve(eq, y(x), ics=...)`, a system `dsolve([eqs], [funcs], ics=...)` | |
| solve_pde | `pdsolve(eq, u(x, t))` | with conditions: `unsupported` |
| sum, product | `summation`, `product` | |
| solve_recurrence | `rsolve(eq, a(n), {a(0): ...})` | the value of `a(n)` |
| limit | `limit(f, x, a, dir)`, both sides `dir='+-'` | |
| series | `series(f, x, a, order)` | `.removeO()` |
| laplace, inverse_laplace | `laplace_transform(..., noconds=True)`, `inverse_laplace_transform` | |
| fourier, inverse_fourier | `fourier_transform` at `k = w/(2 pi)`, `inverse_fourier_transform` of `F(2 pi k)` | SymPy's transform is of `exp(-2 pi i x k)` |
| mellin, inverse_mellin | `mellin_transform(..., noconds=True)`, `inverse_mellin_transform(..., (-oo, oo))` | |
| z_transform, inverse_z_transform | | `unsupported` |
| solve | one equation in one unknown: `solveset(eq, x, Reals or Complexes)`; else `solve(eqs, vars, dict=True)`, real unknowns for `real`; inequalities `reduce_inequalities` | |
| simplify | `simplify` | |
| evaluate | `simplify(expand_func(f.doit()))` | |
| hyperexpand | `hyperexpand` | |
| factor, gcd, lcm, resultant, discriminant, partial_fractions | `factor` (`gaussian=True`), `gcd_list`, `lcm_list`, `resultant`, `discriminant`, `apart` | |
| groebner | `groebner(ps, *vars, order=...)` | the list of polynomials |
| minimal_polynomial | `minimal_polynomial(f, x)` | |
| linear algebra | `Matrix(m).det()`, `.inv()`, `.rank()`, `.nullspace()`, `.eigenvals(multiple=True)`, `.eigenvects()`, `.jordan_form()`, `.gauss_jordan_solve(b)` | of `jordan_form`, J; of `solve_linear`, the solution vector, its parameters `tau0`, `tau1`, ... |
| matrix_power | `Matrix(m)**n`; a determinant with `power`, `simplify((Matrix(m)**n).det())` | |
| holds | `ask(Q.nonnegative(simplify(a - b)))` of `a >= b` (`positive`, `nonpositive`, `negative`, `zero`, `nonzero` of the other relations), the variables real | `True`, `False`, or `None`, a decline |

### Maxima

The answer is `string(result)`, with `display2d: false`.

| op | command | written |
|---|---|---|
| integrate | `integrate(f, x)`, `integrate(f, x, a, b)` | |
| solve_ode | one equation of order 1 or 2: `ode2(eq, y, x)` with `ic1`, `ic2` or `bc2` for the conditions (the function written `y`, not `y(x)`); else `desolve`, with `atvalue` for the conditions | |
| solve_pde | | `unsupported` |
| sum | `simplify_sum(sum(f, n, a, b))` | |
| product | `product(f, n, a, b)` with `simpproduct: true` | |
| solve_recurrence | `solve_rec(eq, a[n], a[0] = ...)` (the function written `a[n]`) | |
| limit | `limit(f, x, a)`, `plus`, `minus` | |
| series | `taylor(f, x, a, order - 1)` | |
| laplace, inverse_laplace | `laplace`, `ilt` | |
| fourier, mellin, z_transform and their inverses | | `unsupported` |
| solve | `solve(eqs, vars)`; `real`: polynomial equations by `algsys` with `realonly: true`, others by `solve`; inequalities `fourier_elim` | |
| simplify | the shortest (as a string) of the expression, `ratsimp`, `fullratsimp`, `trigsimp`, `trigsimp(fullratsimp)`, `radcan` and `factor` of it, as Maxima has no one simplifier | |
| evaluate | `ev(f, nouns)` with `besselexpand: true` | |
| hyperexpand | `hypergeometric_simp` | |
| factor, gcd, lcm, resultant, discriminant, partial_fractions | `factor` (`gfactor` for `gaussian`), `gcd`, `lcm` (`functs`), `resultant`, `poly_discriminant`, `partfrac` | |
| groebner | `poly_reduced_grobner` with `poly_monomial_order` | |
| minimal_polynomial | | `unsupported` |
| linear algebra | `determinant`, `invert`, `rank`, `nullspace`, `eigenvalues`, `eigenvectors`, `dispJordan(jordan(m))`, `linsolve` | of `nullspace`, the basis vectors (`args` of its `span`); of `solve_linear`, the values (`map(rhs, ...)`), its parameters `%r1`, ... |
| matrix_power | `mat_function(lambda([t], t^n), m)` (`diag`); a determinant with `power`, `ratsimp(determinant(...))` of it | |
| holds | `is(a >= b)` with the assumptions, `is(equal(a, b))`, `is(notequal(a, b))` | `true`, `false`, or `unknown`, a decline |

### FriCAS

The answer is `unparse(result::InputForm)`.

| op | command | written |
|---|---|---|
| integrate | `integrate(f, x)`, `integrate(f, x = a..b)` | a definite integral FriCAS cannot do: `"failed"` or `"potentialPole"` |
| solve_ode | one ODE: `solve(eq, y, x)`; with the initial values `y(a), y'(a), ...`: `solve(eq, y, x = a, [values])`; a system, or other conditions: `unsupported` | of a linear ODE, `y(x) = particular + C1 basis.1 + C2 basis.2 + ...` from FriCAS's record; with conditions `y(x) = result`; of a nonlinear one, FriCAS's first integral F as `F = C1` |
| solve_pde, solve_recurrence | | `unsupported` |
| sum, product | `sum(f, n = a..b)`, `product` | infinite bounds: `unsupported` |
| limit | `limit(f, x = a)`, `"right"`, `"left"` | where the sides differ, `[leftHandLimit = ..., rightHandLimit = ...]` |
| series | `approximate(series(f, x = a), order - 1)` | at infinity: `unsupported` |
| laplace, inverse_laplace | `laplace`, `inverseLaplace` | others: `unsupported` |
| solve | `real`: `solve`; `complex`: `radicalSolve` | inequalities: `unsupported` |
| simplify, evaluate | `simplify`, the expression itself | hyperexpand: `unsupported` |
| factor, gcd, lcm, resultant, discriminant | `factor` (over `Complex(Integer)` for `gaussian`), `gcd`, `lcm`, `resultant`, `discriminant` | |
| partial_fractions | `partialFraction(f, x)` | `whole + (num)/(den)^k + ...` from its terms |
| groebner | `groebner` over `DMP` (lex) or `HDMP` (grevlex) | grlex: `unsupported` |
| minimal_polynomial | | `unsupported` |
| linear algebra | `determinant`, `inverse`, `rank`, `nullSpace`, `radicalEigenvalues`, `radicalEigenvectors`, `solve(m, v)` | eigenvectors as `[[value, multiplicity, [vectors]], ...]`; solve_linear as the vector `particular + t1 basis.1 + ...`; jordan_form: `unsupported` |
| matrix_power, holds | | `unsupported`: no power of a symbolic exponent, no decision of a relation; nor a determinant with `power` |

### Giac

The answer is `string(result)`.

| op | command | written |
|---|---|---|
| integrate | `integrate(f, x)`, `integrate(f, x, a, b)` | |
| solve_ode | `desolve(eq, x, y)`, `desolve([eq, conditions], x, y)` | systems: `unsupported` |
| sum, product | `sum(f, n, a, b)`, `product` | |
| solve_recurrence | `rsolve(eq, a(n), conditions)` | |
| limit | `limit(f, x, a)`, `1`, `-1` | |
| series | `convert(series(f, x = a, order - 1), polynom)` | |
| transforms | `laplace`, `ilaplace`, `fourier`, `ifourier`, `ztrans`, `invztrans` (Giac's Fourier transform is the formats' one) | mellin: `unsupported` |
| solve | `real`: `solve`; `complex`: `csolve`; inequalities `solve` | as a plain list, `[op(result)]`: `string` writes solve's as `list[...]` |
| simplify, evaluate | `simplify` | hyperexpand: `unsupported` |
| polynomials | `factor` (`cfactor`), `gcd`, `lcm`, `resultant`, `discriminant`, `partfrac`, `gbasis(ps, vars, plex/tdeg/revlex)`, `pmin(f, x)` | |
| linear algebra | `det`, `inv`, `rank`, `ker`, `[eigenvals(m)]`, `[jordan(m)]`, `jordan`, `linsolve` | eigenvectors as `[P, J]`, the eigenvectors the columns of P, their values the diagonal of J (Giac's `eigenvects` gives P alone); of `jordan_form`, J |
| matrix_power | `matpow(m, n)` (not run); a determinant with `power`, `simplify(det(matpow(m, n)))` | |
| holds | | `unsupported` |

Giac prints its prompts on its standard output and `print` on its standard error; the adapter
starts it as `sh -c 'exec giac --sage 2>&1'`. Set `GIAC_THREADS=1` to keep it to one core.

### Julia

The runner writes Julia code for Symbolics.jl, which a small driver (`src/adapters/julia_driver.jl`)
runs in a fresh module. Julia compiles each function at its first call, which takes minutes for
the solvers (two minutes for `laplace_solve_ode` alone): the environment's package
`julia/MathRunWarmup` runs a small problem of each op when it is precompiled, so that Julia keeps
their compiled code, and the driver loads it and runs the same problems again before the
calibration problems. A session so starts in about a minute (54 s on the Mac at low priority; 11
minutes without the package), and a problem's time is of its work, but for a function that no
earlier problem used, which is compiled within its problem's time. A start may take 15 minutes
before the runner gives up (`math-run version -s julia` shows it). The numbers are given exactly
as Symbolics keeps them: `1/2` as Julia's `1//2`, `sqrt(2)` as `sqrt(Num(2))`, `2^(1/3)` as
Symbolics' term of it (Julia computes all three in floats otherwise), rational arithmetic in big
integers. Symbolics has no assumptions: the problems are given without them, the symbols
Symbolics' own (real). The answer is read from Symbolics' expression tree, each Julia function by
its module and name (`SpecialFunctions.besselj` is `bessel_j`), and written in the neutral syntax
of the formats (section 3); an answer with a function that has no form there is `error`, with the
function in the message (as Symbolics' undocumented `ssqrt`, `scbrt`, `slog` of its solvers, and
`roots_of`). SymbolicIntegration's integral that it cannot do comes back as `∫(f, x)`, written
`integrate(f, x)`; symbolic_solve's integer of a periodic family, an anonymous variable, is written
`k` with `element(k, integers)`. Julia's own floats are written as they are (`taylor` rationalizes
its float coefficients, its default; `simplify` may turn `π` into 3.14159...).

| op | command | written |
|---|---|---|
| integrate | `SymbolicIntegration.integrate(f, x)` (Risch, then rules) | definite integrals: `unsupported` |
| solve_ode | one equation: `symbolic_solve_ode(eq, y, x)` (the function a plain variable `y`); with the initial values `y(0), y'(0), ...` up to the order, `laplace_solve_ode(eq, y, x, [values])` | `y(x) = ...`; a system, or other conditions: `unsupported` |
| limit | `limit(f, x, a, :right/:left/:both)`, of the expressions unwrapped (documented for `log`, `exp` and `+ - * /`; another function is Julia's error) | |
| series | `taylor(f, x, a, 0:order - 1)` | at infinity, or `order <= 0`: `unsupported` |
| laplace, inverse_laplace | `laplace(f, f(t), t, F(s), s)`, `inverse_laplace(F, F(s), s, f(t), t)` | |
| solve | `symbolic_solve(eq, x)`, `symbolic_solve([eqs], [vars])` (over the complex numbers: Symbolics has no real domain) | `[[x = ...], ...]`; inequalities: `unsupported` |
| simplify, evaluate | `simplify`, `simplify(expand_derivatives(f))` | |
| partial_fractions | `partial_frac_decomposition(f, x)` | |
| factor, gcd, lcm | Nemo's `factor`, `gcd`, `lcm` in `polynomial_ring(QQ, [symbols])` (Symbolics has none) | not a polynomial with rational coefficients, or the Gaussian domain: `unsupported` |
| resultant, discriminant | Nemo's `resultant`, `discriminant` in `R[x]`, R the polynomial ring of the other symbols | |
| groebner | `groebner_basis(ps, ordering = Groebner.Lex/DegLex/DegRevLex(vars...))` | |
| minimal_polynomial | Nemo's `minpoly(QQ[x], a)` of the algebraic number in `algebraic_closure(QQ)`: rationals, radicals, `cospi`, `sinpi` | another number (pi, e, gamma(1/4)): `unsupported` |
| determinant, inverse | Julia's `det`, `inv` of a matrix of Symbolics' numbers (exact) | a determinant with `power`: `unsupported` |
| rank, nullspace | Nemo's `rank`, `nullspace` of a matrix of rationals (Symbolics has none) | symbolic entries: `unsupported`; the null space the columns of Nemo's basis |
| solve_linear | `symbolic_linear_solve(M * v .~ b, v)` | as many equations as unknowns only |
| eigenvalues, eigenvectors, jordan_form, matrix_power | | `unsupported`: Symbolics has none |
| sum, product, solve_recurrence, solve_pde, fourier, mellin, z_transform and their inverses, hyperexpand, holds | | `unsupported`: Symbolics has none |

Numeric-fitting integrators (SymbolicNumericIntegration.jl) are not used: a fit is not a solution.
Nor are Symbolics' bridges to SymPy (`sympy_integrate`, ...), which would run SymPy, not Julia.

### Maple (untested)

The answer is `lprint(result)` with `interface(prettyprint = 0, screenwidth = infinity)`. The
assumptions are given with `assuming`. Matrices and vectors are written as lists.

| op | command | written |
|---|---|---|
| integrate | `int(f, x)`, `int(f, x = a..b)` | |
| solve_ode | `dsolve({eqs, conditions}, y(x))`, a system with `{funcs}` | |
| solve_pde | `pdsolve(eq, u(x, t))`, `pdsolve([eq, conditions], u(x, t))` | |
| sum, product, solve_recurrence | `sum`, `product`, `rsolve({eqs, conditions}, a(n))` | |
| limit, series | `limit(f, x = a, right/left)`, `convert(series(f, x = a, order), polynom)` | |
| transforms | `inttrans:-laplace`, `invlaplace`, `fourier`, `invfourier`, `mellin`, `invmellin`, `ztrans`, `invztrans` (inttrans's Fourier transform is the formats' one) | |
| solve | `solve({eqs}, [vars], allsolutions)`, `RealDomain:-solve` for `real` | a list of lists |
| simplify, evaluate, hyperexpand | `simplify`, `simplify(value(f))`, `simplify(convert(f, StandardFunctions))` | |
| polynomials | `factor` (`factor(f, I)`), `gcd`, `lcm`, `resultant`, `discrim`, `convert(f, parfrac, x)`, `Groebner:-Basis(ps, plex/grlex/tdeg(vars))`, `PolynomialTools:-MinimalPolynomial(f, x)` | |
| linear algebra | `LinearAlgebra:-Determinant`, `MatrixInverse`, `Rank`, `NullSpace`, `Eigenvalues`, `Eigenvectors`, `JordanForm`, `LinearSolve` | matrices `convert(m, listlist)`; the null space a list of lists; eigenvectors `[values, the vectors as columns]` |
| matrix_power | `LinearAlgebra:-MatrixPower(Matrix(m), n)`; a determinant with `power`, `simplify(Determinant(MatrixPower(...)))` | `convert(m, listlist)` |
| holds | `is(a >= b) assuming ...`, the variables `::real` | `true`, `false`, or `FAIL` |

Maple's elliptic integrals are of the modulus and the sine of the amplitude: `elliptic_k(m)` is
written `EllipticK(sqrt(m))`, `elliptic_f(phi, m)` `EllipticF(sin(phi), sqrt(m))` (the same for
`-pi/2 <= re(phi) <= pi/2`). The version is `kernelopts(version)`, as
`Maple 2023.0, APPLE ARM64 MACOS, Mar 06 2023, Build ID 1689885`.

### Mathilda (untested)

Mathilda (github.com/stblake/mathilda) is Sam Blake's open-source system in the Wolfram Language.
The runner sends it the Mathematica adapter's problems (below), the same commands, to one session
of `Mathilda` from a pipe. Its answers are Mathematica's `InputForm`, which the checker reads as
Mathematica's. The adapter is written from Mathilda's user documentation, not from its source.

### Mathematica (untested)

The answer is `ToString[result, InputForm, PageWidth -> Infinity]`; the version is `$Version`.

| op | command | written |
|---|---|---|
| integrate | `Integrate[f, x, Assumptions -> ...]` | |
| solve_ode, solve_pde | `DSolve[{eqs, conditions}, {funcs}, vars]` | the rules |
| sum, product, solve_recurrence | `Sum`, `Product`, `RSolve` | |
| limit, series | `Limit[f, x -> a, Direction -> "FromAbove"/"FromBelow"]`, `Normal[Series[f, {x, a, order - 1}]]` | |
| transforms | `LaplaceTransform`, ..., `FourierTransform[..., FourierParameters -> {1, -1}]`, `MellinTransform`, `ZTransform` and their inverses | |
| solve | `Solve[eqs, vars, Reals/Complexes]`; inequalities `Reduce[..., Reals]` | |
| simplify, evaluate, hyperexpand | `FullSimplify[f, assumptions]`, `FunctionExpand` | |
| polynomials | `Factor` (`GaussianIntegers -> True`), `PolynomialGCD`, `PolynomialLCM`, `Resultant`, `Discriminant`, `Apart`, `GroebnerBasis[..., MonomialOrder -> ...]`, `MinimalPolynomial[f, x]` | |
| linear algebra | `Det`, `Inverse`, `MatrixRank`, `NullSpace`, `Eigenvalues`, `Eigensystem`, `JordanDecomposition[m][[2]]`, `Solve[m . v == b, v]` | of `solve_linear`, the vector `v` of unknowns `mrX[1], ...` solved, the free ones its parameters (`LinearSolve` gives one solution) |
| matrix_power | `Simplify[MatrixPower[m, n], Assumptions -> ...]`; a determinant with `power`, `Simplify[Det[MatrixPower[m, n]], ...]` | |
| holds | `Resolve[ForAll[{vars}, assumptions, a >= b], Reals]` | `True` or `False` |

A symbol with `_` is written with `$` (`a_1` as `a$1`), and the single capitals Mathematica keeps
(`C`, `D`, `E`, `I`, `K`, `N`, `O`) with `mr` before them.

## Other commands

| command | |
|---|---|
| `math-run suites` | the site's suites |
| `math-run fetch` | fetch a suite into the cache, checking its hash |
| `math-run version -s <system>` | the system's version, and the seconds it takes to start |
| `math-run translate -s <system> <suite file>` | the code each problem is given as |
| `math-run functions -s <system>` | give the system each function of the formats' table |
| `math-run overhead -s <system>` | the runner's time per problem besides the system's work |
| `math-run machine` | the machine and platform of the header |

## Tests

`cargo test` tests the parser, the printer, the sessions and the dates. `tests/suite/suite.jsonl`
is a small suite of 86 problems, every op and category of the formats, in the suite's format:

```
math-run run -s maxima --suite tests/suite/suite.jsonl --timeout 30 -o /tmp/maxima-test.jsonl
```

## Release

`cargo xtask release` builds the binaries for Linux x86_64, Linux aarch64 (static, musl) and macOS
arm64 into `dist/`, each a `tar.gz` with this README and the licence, and `SHA256SUMS`. The Linux binaries are
cross-compiled with `cargo-zigbuild` (`cargo install cargo-zigbuild`, and zig on the `PATH`); the
macOS one is built on an Apple silicon Mac.

## Licence

Apache License 2.0, see `LICENSE`.
