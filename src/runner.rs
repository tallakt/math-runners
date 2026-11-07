//! A run: the header, the calibration subset and then the subset, one
//! problem at a time (or `--jobs` at a time), each line written as it comes,
//! so that an interrupted run resumes where it stopped.

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::adapters::{self, Adapter};
use crate::problem::Problem;
use crate::suite::{self, Suite};
use crate::worker::{Record, Worker};

/// What a run was started with, kept beside the run file as
/// `<file>.state.json`, so that `resume` needs none of it again.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Options {
    pub system: String,
    pub command: String,
    pub timeout: f64,
    pub jobs: usize,
    pub restart_every: usize,
    pub subset: String,
    /// A local suite file, or none where it is fetched from `site`.
    pub suite_file: Option<PathBuf>,
    pub suite_version: Option<String>,
    pub site: String,
    /// Only these problems (for tests): the upload counts the others as
    /// missing.
    pub only: Option<Vec<String>>,
    pub limit: Option<usize>,
    /// Free text on how the run was made, for the header's `note`.
    #[serde(default)]
    pub note: Option<String>,
    /// Stop a problem whose system holds more memory than this (GB).
    #[serde(default)]
    pub memory_gb: Option<f64>,
}

impl Options {
    /// The memory limit in bytes.
    pub fn memory_limit(&self) -> Option<u64> {
        self.memory_gb
            .filter(|g| *g > 0.0)
            .map(|g| (g * (1u64 << 30) as f64) as u64)
    }
}

fn state_path(out: &Path) -> PathBuf {
    let mut s = out.as_os_str().to_owned();
    s.push(".state.json");
    PathBuf::from(s)
}

pub fn load_suite(o: &Options) -> Result<Suite, String> {
    match &o.suite_file {
        Some(path) => suite::local(path, o.suite_version.as_deref()),
        None => suite::fetch(&o.site, o.suite_version.as_deref(), &o.subset),
    }
}

/// Start a new run, writing to `out`.
pub fn start(o: Options, out: &Path, verbose: bool) -> Result<(), String> {
    if out.exists() {
        return Err(format!(
            "{} exists: resume it with `math-run resume {}`, or remove it",
            out.display(),
            out.display()
        ));
    }
    run(o, out, None, verbose)
}

/// Resume the run of `out` where it stopped.
pub fn resume(out: &Path, verbose: bool) -> Result<(), String> {
    if !out.exists() {
        return Err(format!(
            "{}: no such run file. `math-run resume` takes the file `math-run run` was writing \
             (its -o, or <system>-<subset>-<time>.jsonl)",
            out.display()
        ));
    }
    let state = std::fs::read_to_string(state_path(out)).map_err(|e| {
        format!(
            "{}: {e}. `math-run run` keeps the options of a run in this file beside the run \
             file; without it the run cannot be resumed (start it again with another -o)",
            state_path(out).display()
        )
    })?;
    let o: Options =
        serde_json::from_str(&state).map_err(|e| format!("{}: {e}", state_path(out).display()))?;
    let (header, done) = read_partial(out)?;
    run(o, out, Some((header, done)), verbose)
}

/// The header and the ids answered of a run file; a last line cut short by
/// an interruption is removed.
fn read_partial(out: &Path) -> Result<(Value, HashSet<String>), String> {
    let text = std::fs::read(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let complete = match text.iter().rposition(|&b| b == b'\n') {
        Some(i) => i + 1,
        None => 0,
    };
    if complete < text.len() {
        eprintln!("removing a line cut short at the end of {}", out.display());
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(out)
            .map_err(|e| e.to_string())?;
        f.set_len(complete as u64).map_err(|e| e.to_string())?;
    }
    let mut lines = BufReader::new(&text[..complete]).lines();
    let header: Value = lines
        .next()
        .ok_or("the run file has no header")?
        .map_err(|e| e.to_string())
        .and_then(|l| serde_json::from_str(&l).map_err(|e| e.to_string()))?;
    let mut done = HashSet::new();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let v: Value =
            serde_json::from_str(&line).map_err(|e| format!("{}: {e}", out.display()))?;
        if let Some(id) = v["id"].as_str() {
            done.insert(id.to_string());
        }
    }
    Ok((header, done))
}

fn run(
    o: Options,
    out: &Path,
    resumed: Option<(Value, HashSet<String>)>,
    verbose: bool,
) -> Result<(), String> {
    let adapter =
        adapters::by_slug(&o.system).ok_or_else(|| format!("no adapter for {}", o.system))?;
    // the system's version, from a first session, which the first job keeps;
    // before the suite is fetched, which is large, and anything is written
    let mut first = Worker::new(adapter.as_ref(), &o.command, o.restart_every);
    first.verbose = verbose;
    first.memory_limit = o.memory_limit();
    first.ensure_started()?;
    let version = first.version.clone().unwrap();
    let suite = load_suite(&o)?;
    let mut todo = suite::ordered(&suite.problems, &o.subset);
    if let Some(only) = &o.only {
        todo.retain(|p| only.contains(&p.id));
    }
    if let Some(n) = o.limit {
        let calibration = todo.iter().filter(|p| p.calibration).count();
        todo.truncate(calibration + n);
    }
    let total = todo.len();
    if !adapter.tested() {
        eprintln!(
            "note: the {} adapter was written from the documentation, untested",
            adapter.name()
        );
    }
    eprintln!(
        "{} {version}, started in {:.2} s; {total} problems, timeout {} s, {} job(s)",
        adapter.name(),
        first.start_times[0],
        o.timeout,
        o.jobs
    );

    let (mut header, done) = match resumed {
        Some((h, done)) => {
            check_resume(&h, &suite, &o, adapter.as_ref(), &version)?;
            (h, done)
        }
        None => (
            header(&suite, &o, adapter.as_ref(), &version),
            HashSet::new(),
        ),
    };
    if !done.is_empty() && todo.iter().all(|p| done.contains(&p.id)) {
        eprintln!("every problem is answered: nothing to resume");
        return Ok(());
    }
    if !done.is_empty() {
        let n = header["resumed"].as_u64().unwrap_or(0) + 1;
        header["resumed"] = json!(n);
        header.as_object_mut().unwrap().remove("finished");
        eprintln!("resuming: {} of {total} answered", done.len());
    }
    todo.retain(|p| !done.contains(&p.id));
    eprintln!(
        "writing {}; Ctrl-C stops the run, and `math-run resume {}` goes on where it stopped",
        out.display(),
        out.display()
    );
    if let Some(id) = header["machine"]["id"].as_str() {
        eprintln!(
            "machine id {id}, a salted hash of this machine's own id, goes in the upload \
             (MATH_RUN_NO_MACHINE_ID=1 leaves it out)"
        );
    }
    let mut file = if done.is_empty() && !out.exists() {
        // the options are kept once the run is sure to begin
        std::fs::write(state_path(out), serde_json::to_string_pretty(&o).unwrap())
            .map_err(|e| format!("{}: {e}", state_path(out).display()))?;
        let mut f = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
        writeln!(f, "{header}").map_err(|e| e.to_string())?;
        f
    } else {
        std::fs::OpenOptions::new()
            .append(true)
            .open(out)
            .map_err(|e| e.to_string())?
    };

    let queue = Arc::new(Mutex::new(todo.into_iter().collect::<VecDeque<Problem>>()));
    let (tx, rx) = channel::<Record>();
    let mut answered = done.len();
    let started = Instant::now();
    let mut counts = std::collections::BTreeMap::<&'static str, usize>::new();
    std::thread::scope(|scope| {
        let mut workers = vec![first];
        for _ in 1..o.jobs.max(1) {
            let mut w = Worker::new(adapter.as_ref(), &o.command, o.restart_every);
            w.verbose = verbose;
            w.memory_limit = o.memory_limit();
            workers.push(w);
        }
        for mut w in workers {
            let queue = queue.clone();
            let tx = tx.clone();
            let timeout = o.timeout;
            scope.spawn(move || {
                loop {
                    let next = queue.lock().unwrap().pop_front();
                    let Some(p) = next else { break };
                    let r = w.solve(&p, timeout);
                    if tx.send(r).is_err() {
                        break;
                    }
                }
                w.stop();
            });
        }
        drop(tx);
        for r in rx {
            answered += 1;
            *counts.entry(r.status).or_default() += 1;
            let line = serde_json::to_string(&r).unwrap();
            if let Err(e) = writeln!(file, "{line}").and_then(|_| file.flush()) {
                eprintln!("cannot write {}: {e}", out.display());
                std::process::exit(1);
            }
            let _ = writeln!(
                std::io::stderr(),
                "{answered:>6}/{total}  {:<11} {:>9}  {}{}",
                r.status,
                r.wall.map(|w| format!("{w:.3}s")).unwrap_or_default(),
                r.id,
                r.message
                    .as_deref()
                    .map(|m| format!(
                        "  {}",
                        m.lines()
                            .next()
                            .unwrap_or("")
                            .chars()
                            .take(100)
                            .collect::<String>()
                    ))
                    .unwrap_or_default()
            );
        }
    });
    drop(file);
    header["finished"] = json!(now_iso());
    rewrite_header(out, &header)?;
    eprintln!(
        "done in {:.0} s: {}; written to {}",
        started.elapsed().as_secs_f64(),
        counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect::<Vec<_>>()
            .join(", "),
        out.display()
    );
    Ok(())
}

fn header(suite: &Suite, o: &Options, adapter: &dyn Adapter, version: &str) -> Value {
    let timeout = if o.timeout.fract() == 0.0 {
        json!(o.timeout as u64)
    } else {
        json!(o.timeout)
    };
    let mut h = json!({
        "format": "math-upload/1",
        "suite": suite.version,
        "suite_sha256": suite.sha256,
        "subset": o.subset,
        "system": {"name": adapter.name(), "version": version},
        "runner": {"name": "math-run", "version": env!("CARGO_PKG_VERSION")},
        "platform": crate::machine::platform(),
        "machine": crate::machine::detect(),
        "timeout": timeout,
        "started": now_iso(),
        "jobs": o.jobs,
    });
    if let Some(note) = o.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        h["note"] = json!(note);
    }
    h
}

fn check_resume(
    h: &Value,
    suite: &Suite,
    o: &Options,
    adapter: &dyn Adapter,
    version: &str,
) -> Result<(), String> {
    let same = |k: &str, v: &Value| -> Result<(), String> {
        if h.pointer(k) == Some(v) {
            Ok(())
        } else {
            Err(format!(
                "cannot resume: the run's {k} is {}, now {v}",
                h.pointer(k).unwrap_or(&Value::Null)
            ))
        }
    };
    same("/suite", &json!(suite.version))?;
    same("/suite_sha256", &json!(suite.sha256))?;
    same("/subset", &json!(o.subset))?;
    same("/system/name", &json!(adapter.name()))?;
    same("/system/version", &json!(version))?;
    if h["timeout"].as_f64() != Some(o.timeout) {
        return Err("cannot resume: the timeout differs".into());
    }
    let m = crate::machine::detect();
    if h["machine"]["cpu"].as_str() != Some(m.cpu.as_str())
        || h["machine"]["id"].as_str() != m.id.as_deref()
    {
        eprintln!(
            "warning: the run was started on {}, this machine is {}",
            h["machine"]["cpu"], m.cpu
        );
    }
    Ok(())
}

/// Write the header again, as the first line, with `finished`.
fn rewrite_header(out: &Path, header: &Value) -> Result<(), String> {
    let tmp = out.with_extension("rewrite.tmp");
    {
        let src = std::fs::File::open(out).map_err(|e| e.to_string())?;
        let mut lines = BufReader::new(src).lines();
        lines.next();
        let mut dst =
            std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
        writeln!(dst, "{header}").map_err(|e| e.to_string())?;
        for line in lines {
            writeln!(dst, "{}", line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }
        dst.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())
}

/// Now, as `2026-11-04T10:00:00Z`.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    iso(secs)
}

pub fn iso(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // days to the civil date (H. Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso_dates() {
        assert_eq!(super::iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::iso(1793786400), "2026-11-04T10:00:00Z");
        assert_eq!(super::iso(951782400), "2000-02-29T00:00:00Z");
    }
}
