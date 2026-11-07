//! A worker gives problems to one session of a system, one at a time, and
//! turns what the system printed into the line of the upload.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::adapters::Adapter;
use crate::problem::Problem;
use crate::session::{ReadError, Session, Spec, cut, tail};

/// A line of the upload's answers (section 5.2).
#[derive(Serialize, Clone, Debug)]
pub struct Record {
    pub id: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Record {
    fn new(id: &str, status: &'static str) -> Record {
        Record {
            id: id.to_string(),
            status,
            answer: None,
            wall: None,
            cpu: None,
            message: None,
        }
    }
}

/// The most an answer may be (section 5.2).
pub const MAX_ANSWER: usize = 1_000_000;

/// How long the system may take to reset its state before a problem.
const RESET_TIME: Duration = Duration::from_secs(120);

/// How long the system may take to start.
const START_TIME: Duration = Duration::from_secs(300);

/// How long the system may print nothing and compute nothing while it
/// starts, before it is taken as not being the system.
const START_IDLE: Duration = Duration::from_secs(20);

pub struct Worker<'a> {
    pub adapter: &'a dyn Adapter,
    pub spec: Spec,
    session: Option<Session>,
    nonce: u64,
    tag: String,
    /// The version, as read at the first start.
    pub version: Option<String>,
    /// Start the session again after this many problems, 0 never.
    pub restart_every: usize,
    since_start: usize,
    /// The seconds each start took, to report the overhead.
    pub start_times: Vec<f64>,
    pub verbose: bool,
    /// The most memory (bytes) the system may hold, none: no limit.
    pub memory_limit: Option<u64>,
    private: crate::scrub::Private,
}

impl<'a> Worker<'a> {
    pub fn new(adapter: &'a dyn Adapter, program: &str, restart_every: usize) -> Worker<'a> {
        let tag = format!(
            "{:x}",
            std::process::id() as u64 * 7919 + now_nanos() % 7919
        );
        Worker {
            adapter,
            spec: adapter.spec(program),
            session: None,
            nonce: 0,
            tag,
            version: None,
            restart_every,
            since_start: 0,
            start_times: Vec::new(),
            verbose: false,
            memory_limit: None,
            private: crate::scrub::Private::here(),
        }
    }

    fn next_nonce(&mut self) -> String {
        self.nonce += 1;
        format!("{}-{}", self.tag, self.nonce)
    }

    /// Start the system, unless it runs; read its version.
    pub fn ensure_started(&mut self) -> Result<(), String> {
        if self.session.is_some() {
            return Ok(());
        }
        let t0 = Instant::now();
        let mut s = Session::spawn(&self.spec).map_err(|e| e.to_string())?;
        let nonce = self.next_nonce();
        let text = self.adapter.start(&nonce);
        s.send(&text)
            .map_err(|e| format!("cannot write to the system: {e}"))?;
        // A program that is not a computer algebra system (`cat`, a shell)
        // waits for input and computes nothing: it is not waited for until
        // START_TIME is over. The system itself may be slow to load, but it
        // reads or computes meanwhile.
        s.set_idle(START_IDLE);
        let before = s
            .read_to(
                &format!("@@MR-READY {nonce}"),
                Instant::now() + START_TIME,
                true,
            )
            .inspect(|_| s.set_idle(Duration::from_secs(3)))
            .map_err(|e| {
                let err = s.take_stderr();
                let what = match e {
                    ReadError::Timeout => format!(
                        "did not answer within {} s (is `{}` the system's command-line program?)",
                        START_TIME.as_secs(),
                        self.spec.program
                    ),
                    // the tail of what it printed is in the error already
                    ReadError::Ended(_) => format!("{e}"),
                    ReadError::Waiting(_) => format!(
                        "{e} (it printed nothing and computed nothing for {} s: is `{}` the \
                         system's own command-line program?)",
                        START_IDLE.as_secs(),
                        self.spec.program
                    ),
                    _ => format!("{e} {}", tail(&err)),
                };
                let hint = if what.contains("No module named") && self.adapter.slug() == "sympy" {
                    ". SymPy is not installed for this Python: install it (`apt install \
                     python3-sympy`, or `pip install sympy` in a virtual environment) and, for \
                     a virtual environment's Python, give it with --command"
                } else {
                    ""
                };
                format!("the system did not start: {what}{hint}")
            })?;
        let version = before
            .lines()
            .find_map(|l| l.trim().strip_prefix("@@MR-VERSION "))
            .map(|v| self.adapter.clean_version(v))
            .ok_or_else(|| {
                format!(
                    "the system gave no version (is `{}` the system's own command-line \
                     program?): {}",
                    self.spec.program,
                    tail(&before)
                )
            })?;
        match &self.version {
            Some(v) if *v != version => {
                return Err(format!(
                    "the system's version changed from {v} to {version}"
                ));
            }
            _ => self.version = Some(version),
        }
        s.take_stderr();
        s.set_memory_limit(self.memory_limit);
        self.start_times.push(t0.elapsed().as_secs_f64());
        self.session = Some(s);
        self.since_start = 0;
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut s) = self.session.take() {
            s.close(self.adapter.goodbye());
        }
    }

    fn kill(&mut self) {
        if let Some(mut s) = self.session.take() {
            s.kill();
        }
    }

    /// Give the system one problem, with `timeout` seconds. A message is
    /// scrubbed of what is private here (paths, user and host names).
    pub fn solve(&mut self, p: &Problem, timeout: f64) -> Record {
        let mut r = self.solve_unscrubbed(p, timeout);
        if let Some(m) = r.message.take() {
            r.message = Some(self.private.scrub(&m));
        }
        r
    }

    fn solve_unscrubbed(&mut self, p: &Problem, timeout: f64) -> Record {
        let nonce = self.next_nonce();
        let script = match self.adapter.problem(p, &nonce) {
            Ok(s) => s,
            Err(u) => {
                let mut r = Record::new(&p.id, "unsupported");
                r.message = Some(u.0);
                return r;
            }
        };
        if self.restart_every > 0 && self.since_start >= self.restart_every {
            self.stop();
        }
        // two tries: a session that broke since the last problem is started
        // again
        let mut last_error = String::new();
        for _ in 0..2 {
            if let Err(e) = self.ensure_started() {
                last_error = e;
                self.kill();
                continue;
            }
            match self.attempt(p, &script, &nonce, timeout) {
                Ok(r) => {
                    self.since_start += 1;
                    return r;
                }
                Err(e) => {
                    last_error = e;
                    self.kill();
                }
            }
        }
        let mut r = Record::new(&p.id, "error");
        r.message = Some(format!(
            "the runner could not give the problem: {last_error}"
        ));
        r
    }

    /// One try: `Err` where the session failed before the problem began.
    fn attempt(
        &mut self,
        p: &Problem,
        script: &str,
        nonce: &str,
        timeout: f64,
    ) -> Result<Record, String> {
        let s = self.session.as_mut().unwrap();
        s.take_stderr();
        s.send(script)
            .map_err(|e| format!("cannot write to the system: {e}"))?;
        s.read_to(
            &format!("@@MR-BEGIN {nonce}"),
            Instant::now() + RESET_TIME,
            false,
        )
        .map_err(|e| format!("the reset failed: {e}"))?;
        let t0 = Instant::now();
        let cpu0 = s.cpu_seconds();
        let deadline = t0 + Duration::from_secs_f64(timeout);
        // A system with no clock of its own prints `@@MR-COMPUTED` as its
        // work is done, before it prints the answer: the times are taken
        // then, here, the CPU time from the operating system.
        let computed = format!("@@MR-COMPUTED {nonce}");
        let end = format!("@@MR-END {nonce}");
        let mut taken: Option<(f64, Option<f64>)> = None;
        let mut before = String::new();
        let mut result = s
            .read_to_any(&[&computed, &end], deadline, true)
            .map(|(which, text)| {
                if which == 0 {
                    taken = Some((t0.elapsed().as_secs_f64(), s.cpu_seconds()));
                }
                (which, text)
            });
        if let Ok((0, text)) = &result {
            before = text.clone();
            result = s.read_to(&end, deadline, true).map(|t| (1, t));
        }
        let result = result.map(|(_, text)| before + &text);
        let (wall, cpu1) = taken.unwrap_or_else(|| (t0.elapsed().as_secs_f64(), s.cpu_seconds()));
        let cpu = match (cpu0, cpu1) {
            (Some(a), Some(b)) if b >= a => Some(b - a),
            _ => None,
        };
        let stderr = s.take_stderr();
        let mut r = Record::new(&p.id, "error");
        match result {
            Ok(text) => {
                self.read_result(&mut r, &text, &stderr, nonce, wall, cpu);
                if self.verbose {
                    eprintln!("{}: {}", p.id, text.trim());
                }
            }
            Err(ReadError::Timeout) => {
                r.status = "timeout";
                r.wall = Some(round(wall));
                r.cpu = cpu.map(round);
                self.kill();
            }
            Err(e) => {
                r.wall = Some(round(wall));
                r.cpu = cpu.map(round);
                r.message = Some(cut(&format!("{e}"), 2000));
                self.kill();
            }
        }
        Ok(r)
    }

    fn read_result(
        &self,
        r: &mut Record,
        text: &str,
        stderr: &str,
        nonce: &str,
        wall: f64,
        cpu: Option<f64>,
    ) {
        let mut answer: Option<Vec<&str>> = None;
        let mut in_answer = false;
        let mut errors = Vec::new();
        let mut noise = Vec::new();
        let mut time: Option<(f64, f64)> = None;
        let begin = format!("@@MR-ANSWER {nonce}");
        let end = format!("@@MR-ANSWER-END {nonce}");
        for line in text.split('\n') {
            let t = line.trim_end_matches('\r');
            if in_answer {
                if t.trim() == end {
                    in_answer = false;
                } else {
                    answer.as_mut().unwrap().push(t);
                }
            } else if t.trim() == begin {
                in_answer = true;
                answer = Some(Vec::new());
            } else if let Some(rest) = t.trim().strip_prefix("@@MR-TIME ") {
                let v: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect();
                if v.len() == 2 {
                    time = Some((v[0], v[1]));
                }
            } else if let Some(rest) = t.trim().strip_prefix("@@MR-ERROR") {
                errors.push(rest.trim().to_string());
            } else {
                noise.push(t);
            }
        }
        let (c, w) = match time {
            Some((c, w)) => (Some(c), w),
            None => (cpu, wall),
        };
        r.wall = Some(round(w));
        r.cpu = c.map(round);
        match answer {
            Some(lines) if !in_answer && errors.is_empty() => {
                let a = self.adapter.clean_answer(&lines.join("\n"));
                if a.is_empty() {
                    r.message = Some("the system's answer is empty".into());
                } else if a.len() > MAX_ANSWER {
                    r.message = Some(format!("the answer is over 1 MB ({} bytes)", a.len()));
                } else {
                    r.status = "ok";
                    r.answer = Some(a);
                }
            }
            _ => {
                let noise = self.adapter.clean_noise(&noise.join("\n"));
                let mut msg: Vec<String> = errors.into_iter().filter(|e| !e.is_empty()).collect();
                if !noise.is_empty() {
                    msg.push(noise);
                }
                let stderr = stderr.trim();
                if msg.is_empty() && !stderr.is_empty() {
                    msg.push(stderr.to_string());
                }
                if msg.is_empty() {
                    msg.push("the system gave no answer".into());
                }
                r.message = Some(cut(&msg.join("\n"), 2000));
            }
        }
    }
}

impl Drop for Worker<'_> {
    fn drop(&mut self) {
        self.stop();
    }
}

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Seconds to the microsecond.
pub fn round(s: f64) -> f64 {
    (s.max(0.0) * 1e6).round() / 1e6
}
