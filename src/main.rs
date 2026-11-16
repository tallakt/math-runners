//! math-run: gives the suite of math.osp.no to a computer algebra system,
//! times it, and writes and uploads the run file. See the README, and the
//! formats at https://math.osp.no/formats.

mod adapters;
mod expr;
mod functions;
mod machine;
mod net;
mod print;
mod problem;
mod procinfo;
mod runner;
mod scrub;
mod session;
mod suite;
mod worker;

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};
use serde_json::json;

use crate::adapters::Adapter;
use crate::problem::Problem;

#[derive(Parser)]
#[command(
    name = "math-run",
    version,
    about = "Run the suite of math.osp.no on a computer algebra system"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args, Clone)]
struct SystemArgs {
    /// The system: sympy, maxima, fricas, giac, julia, maple, mathematica or mathilda.
    #[arg(long, short)]
    system: String,
    /// The program that starts it, where it is not the usual one on the
    /// PATH (python3, maxima, fricas, giac, julia, maple, wolfram, Mathilda).
    #[arg(long)]
    command: Option<String>,
}

#[derive(clap::Args, Clone)]
struct SuiteArgs {
    /// A local suite file, plain or gzipped, instead of fetching the suite.
    #[arg(long)]
    suite: Option<PathBuf>,
    /// The suite's version: of a local file, where no manifest.json is
    /// beside it; of the site, where not the newest.
    #[arg(long)]
    suite_version: Option<String>,
    /// The site.
    #[arg(long, default_value = net::SITE)]
    site: String,
    /// The subset: full or quick.
    #[arg(long, default_value = "full")]
    subset: String,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the calibration subset, then the subset, and write the run file.
    Run {
        #[command(flatten)]
        system: SystemArgs,
        #[command(flatten)]
        suite: SuiteArgs,
        /// The seconds each problem has.
        #[arg(long, default_value_t = 30.0)]
        timeout: f64,
        /// Problems at a time, each in a session of its own. The reference
        /// runs use 1, for fixed timing.
        #[arg(long, default_value_t = 1)]
        jobs: usize,
        /// Start the system again after this many problems (0: never).
        #[arg(long, default_value_t = 500)]
        restart_every: usize,
        /// Stop a problem whose system holds more than this many GB of
        /// memory together (checked four times a second; 0 or not given:
        /// no limit). The problem is an `error`.
        #[arg(long)]
        memory_gb: Option<f64>,
        /// The run file to write.
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Only these problems, by id, comma separated (for tests).
        #[arg(long, value_delimiter = ',')]
        only: Option<Vec<String>>,
        /// Only the calibration subset and this many problems more (for tests).
        #[arg(long)]
        limit: Option<usize>,
        /// Print what the system prints for each problem.
        #[arg(long, short)]
        verbose: bool,
        /// A note on how the run was made, where the header does not say
        /// (at most 2000 bytes): the run's page on the site shows it.
        #[arg(long)]
        note: Option<String>,
    },
    /// Resume an interrupted run.
    Resume {
        file: PathBuf,
        #[arg(long, short)]
        verbose: bool,
    },
    /// Upload a run file: POST /api/runs.
    Upload {
        file: PathBuf,
        /// The upload token, from the site's /account (after registering at
        /// /register); or $MATH_OSP_TOKEN, which keeps it out of the shell's
        /// history and the process list.
        #[arg(long, env = "MATH_OSP_TOKEN", hide_env_values = true)]
        token: Option<String>,
        #[arg(long, default_value = net::SITE)]
        site: String,
        /// Set the run's note in the header before uploading (the file is
        /// not changed).
        #[arg(long)]
        note: Option<String>,
        /// Upload a run that has not finished (it was interrupted, and is not
        /// resumed): the problems it did not reach count as missing.
        #[arg(long)]
        unfinished: bool,
        /// Upload a file that was uploaded already: a second run on the site.
        #[arg(long)]
        again: bool,
    },
    /// The status of an uploaded run: GET /api/runs/<run>.
    Status {
        run: String,
        #[arg(long, env = "MATH_OSP_TOKEN", hide_env_values = true)]
        token: Option<String>,
        #[arg(long, default_value = net::SITE)]
        site: String,
    },
    /// List the site's suites.
    Suites {
        #[arg(long, default_value = net::SITE)]
        site: String,
    },
    /// Fetch a suite into the cache, checking its hash.
    Fetch {
        #[command(flatten)]
        suite: SuiteArgs,
    },
    /// Start the system, and print its version and the seconds it took.
    Version {
        #[command(flatten)]
        system: SystemArgs,
    },
    /// Print the code given to the system for each problem of a suite file.
    Translate {
        #[command(flatten)]
        system: SystemArgs,
        suite: PathBuf,
        /// Only these problems.
        #[arg(long, value_delimiter = ',')]
        only: Option<Vec<String>>,
    },
    /// Give the system each function of the formats' table, to see which it
    /// knows.
    Functions {
        #[command(flatten)]
        system: SystemArgs,
        #[arg(long, default_value_t = 30.0)]
        timeout: f64,
    },
    /// Measure the runner's overhead per problem: a trivial problem, many
    /// times.
    Overhead {
        #[command(flatten)]
        system: SystemArgs,
        #[arg(long, default_value_t = 50)]
        n: usize,
    },
    /// Print the machine and platform of the header.
    Machine,
}

fn adapter(s: &SystemArgs) -> Result<(Box<dyn Adapter>, String), String> {
    let a = adapters::by_slug(&s.system).ok_or_else(|| {
        format!(
            "no system {}: one of {}",
            s.system,
            adapters::all()
                .iter()
                .map(|a| a.slug())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let cmd = s
        .command
        .clone()
        .unwrap_or_else(|| a.default_command().to_string());
    Ok((a, cmd))
}

fn main() {
    session::install_signal_handlers();
    let cli = Cli::parse();
    if let Err(e) = real_main(cli) {
        eprintln!("math-run: {e}");
        std::process::exit(1);
    }
}

fn real_main(cli: Cli) -> Result<(), String> {
    match cli.cmd {
        Cmd::Run {
            system,
            suite,
            timeout,
            jobs,
            restart_every,
            memory_gb,
            out,
            only,
            limit,
            verbose,
            note,
        } => {
            let (a, command) = adapter(&system)?;
            if !["full", "quick"].contains(&suite.subset.as_str()) {
                return Err("the subset is full or quick".into());
            }
            if timeout <= 0.0 {
                return Err("the timeout is a number of seconds".into());
            }
            let o = runner::Options {
                system: a.slug().to_string(),
                command,
                timeout,
                jobs: jobs.max(1),
                restart_every,
                subset: suite.subset.clone(),
                suite_file: suite
                    .suite
                    .as_ref()
                    .map(|p| std::fs::canonicalize(p).unwrap_or(p.clone())),
                suite_version: suite.suite_version.clone(),
                site: suite.site.clone(),
                only,
                limit,
                note,
                memory_gb,
            };
            let out = out.unwrap_or_else(|| {
                PathBuf::from(format!(
                    "{}-{}-{}.jsonl",
                    a.slug(),
                    suite.subset,
                    runner::now_iso().replace(':', "")
                ))
            });
            runner::start(o, &out, verbose)
        }
        Cmd::Resume { file, verbose } => runner::resume(&file, verbose),
        Cmd::Upload {
            file,
            token,
            site,
            note,
            unfinished,
            again,
        } => net::upload_run(
            &site,
            token.as_deref(),
            &file,
            note.as_deref(),
            unfinished,
            again,
        ),
        Cmd::Status { run, token, site } => net::status_of(&site, token.as_deref(), &run),
        Cmd::Suites { site } => {
            let v = net::get_json(&format!("{}/api/suites", site.trim_end_matches('/')))?;
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            Ok(())
        }
        Cmd::Fetch { suite } => {
            let s = suite::fetch(&suite.site, suite.suite_version.as_deref(), &suite.subset)?;
            println!(
                "{} {} problems, sha256 {}",
                s.version,
                s.problems.len(),
                s.sha256
            );
            Ok(())
        }
        Cmd::Version { system } => {
            let (a, cmd) = adapter(&system)?;
            let mut w = worker::Worker::new(a.as_ref(), &cmd, 0);
            w.ensure_started()?;
            println!("{} {}", a.name(), w.version.clone().unwrap());
            if !a.tested() {
                println!(
                    "(the {} adapter was written from the documentation, untested)",
                    a.name()
                );
            }
            println!("started in {:.3} s", w.start_times[0]);
            Ok(())
        }
        Cmd::Translate {
            system,
            suite,
            only,
        } => {
            let (a, _) = adapter(&system)?;
            for p in suite::read_problems(&suite)? {
                if only.as_ref().is_some_and(|o| !o.contains(&p.id)) {
                    continue;
                }
                println!("# {}  ({})", p.id, p.op);
                let text = if a.slug() == "sympy" {
                    adapters::sympy::show(&p)
                } else if a.slug() == "julia" {
                    adapters::julia::show(&p)
                } else {
                    a.problem(&p, "0")
                };
                match text {
                    Ok(t) => println!("{}\n", t.trim_end()),
                    Err(u) => println!("unsupported: {u}\n"),
                }
            }
            Ok(())
        }
        Cmd::Functions { system, timeout } => {
            let (a, cmd) = adapter(&system)?;
            let mut w = worker::Worker::new(a.as_ref(), &cmd, 0);
            for &(name, arity) in functions::SIGNATURES {
                let call = functions::sample_call(name, arity);
                let p = Problem::from_json(&json!({
                    "id": format!("{name}-{arity}"),
                    "category": "special_function",
                    "problem": {"op": "evaluate", "args": {"expr": call}},
                }))?;
                let r = w.solve(&p, timeout);
                let what = r.answer.or(r.message).unwrap_or_default();
                println!(
                    "{:<18} {:<11} {}",
                    format!("{name}/{arity}"),
                    r.status,
                    session::cut(&what.replace('\n', " "), 110)
                );
            }
            Ok(())
        }
        Cmd::Overhead { system, n } => {
            let (a, cmd) = adapter(&system)?;
            let mut w = worker::Worker::new(a.as_ref(), &cmd, 0);
            w.ensure_started()?;
            let p = Problem::from_json(&json!({
                "id": "overhead",
                "category": "simplification",
                "problem": {"op": "factor", "args": {"expr": "x^2 - 1"}},
            }))?;
            let mut inside = 0.0;
            let t0 = Instant::now();
            for _ in 0..n {
                let r = w.solve(&p, 60.0);
                if r.status != "ok" {
                    return Err(format!("the trivial problem failed: {:?}", r.message));
                }
                inside += r.wall.unwrap_or(0.0);
            }
            let total = t0.elapsed().as_secs_f64();
            println!(
                "{} {}: start-up {:.3} s; per problem {:.4} s in all, {:.4} s timed in the system, {:.4} s overhead",
                a.name(),
                w.version.clone().unwrap(),
                w.start_times[0],
                total / n as f64,
                inside / n as f64,
                (total - inside) / n as f64
            );
            Ok(())
        }
        Cmd::Machine => {
            println!("platform: {}", machine::platform());
            println!(
                "machine: {}",
                serde_json::to_string(&machine::detect()).unwrap()
            );
            Ok(())
        }
    }
}
