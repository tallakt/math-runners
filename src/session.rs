//! A system's process: started once and kept, fed the problems through its
//! standard input, read through its standard output, killed and started
//! again where a problem runs out of time.
//!
//! The system runs in a process group of its own, so that the runner kills
//! every process it starts (as Maple's kernel, or FriCAS's Lisp). Its CPU
//! time is read from the operating system for the whole group, for a
//! problem that runs out of time, and to tell a system that waits for input
//! (as Maxima asking "Is a positive?") from one that computes.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

/// The most output read for one problem, which is cut there: an answer is
/// at most 1 MB, and a system that prints more is stopped.
pub const MAX_OUTPUT: usize = 16 * 1024 * 1024;

/// How long a system may sit with no output and no CPU time before it is
/// taken as waiting for input.
const IDLE: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct Spec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub dir: Option<std::path::PathBuf>,
}

enum Event {
    Out(Vec<u8>),
    Err(Vec<u8>),
    Closed,
}

#[derive(Debug)]
pub enum ReadError {
    /// The deadline passed.
    Timeout,
    /// The process closed its output: it ended.
    Ended(String),
    /// No output and no CPU time for a while: it waits for input.
    Waiting(String),
    /// It printed more than `MAX_OUTPUT`.
    TooMuch,
    /// Its processes held more memory than `--memory-gb` allows (the limit,
    /// in bytes).
    Memory(u64),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Timeout => write!(f, "out of time"),
            ReadError::Ended(tail) => write!(f, "the system ended: {tail}"),
            ReadError::Waiting(tail) => write!(f, "the system waits for input: {tail}"),
            ReadError::TooMuch => write!(f, "the system printed over 16 MB"),
            ReadError::Memory(b) => write!(
                f,
                "out of memory: the system held over {:.2} GB, the limit of --memory-gb",
                *b as f64 / (1u64 << 30) as f64
            ),
        }
    }
}

pub struct Session {
    child: Child,
    pgid: i32,
    stdin: Option<ChildStdin>,
    rx: Receiver<Event>,
    out: String,
    err: String,
    closed: usize,
    /// The most memory (bytes) the system's processes may hold together.
    memory_limit: Option<u64>,
    /// How long it may sit idle before it is taken as waiting for input.
    idle: Duration,
}

impl Session {
    pub fn spawn(spec: &Spec) -> std::io::Result<Session> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &spec.dir {
            cmd.current_dir(dir);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| std::io::Error::new(e.kind(), spawn_message(&spec.program, &e)))?;
        let pgid = child.id() as i32;
        register(pgid);
        let (tx, rx) = channel();
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let tx2 = tx.clone();
        thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Event::Closed);
                        return;
                    }
                    Ok(n) => {
                        if tx.send(Event::Out(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match stderr.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx2.send(Event::Closed);
                        return;
                    }
                    Ok(n) => {
                        if tx2.send(Event::Err(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        let stdin = child.stdin.take();
        Ok(Session {
            child,
            pgid,
            stdin,
            rx,
            out: String::new(),
            err: String::new(),
            closed: 0,
            memory_limit: None,
            idle: IDLE,
        })
    }

    /// How long the system may print nothing and compute nothing before it is
    /// taken as waiting for input (3 s unless set).
    pub fn set_idle(&mut self, idle: Duration) {
        self.idle = idle;
    }

    /// Stop the system (`ReadError::Memory`) when its processes hold more
    /// than this many bytes together.
    pub fn set_memory_limit(&mut self, bytes: Option<u64>) {
        self.memory_limit = bytes;
    }

    pub fn send(&mut self, text: &str) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("the system's input is closed"))?;
        stdin.write_all(text.as_bytes())?;
        if !text.ends_with('\n') {
            stdin.write_all(b"\n")?;
        }
        stdin.flush()
    }

    /// Take the standard error printed so far.
    pub fn take_stderr(&mut self) -> String {
        self.drain();
        std::mem::take(&mut self.err)
    }

    fn drain(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.take(ev);
        }
    }

    fn take(&mut self, ev: Event) {
        match ev {
            Event::Out(b) => self.out.push_str(&String::from_utf8_lossy(&b)),
            Event::Err(b) => {
                if self.err.len() < MAX_OUTPUT {
                    self.err.push_str(&String::from_utf8_lossy(&b));
                }
            }
            Event::Closed => self.closed += 1,
        }
    }

    /// Read up to the line that is `marker` alone, and return what came
    /// before it, taken from the buffer. With `watch_idle`, a system that
    /// prints nothing and computes nothing for a while is taken as waiting
    /// for input.
    pub fn read_to(
        &mut self,
        marker: &str,
        deadline: Instant,
        watch_idle: bool,
    ) -> Result<String, ReadError> {
        self.read_to_any(&[marker], deadline, watch_idle)
            .map(|(_, text)| text)
    }

    /// Read up to the first line that is one of `markers` alone: which one,
    /// and what came before it.
    pub fn read_to_any(
        &mut self,
        markers: &[&str],
        deadline: Instant,
        watch_idle: bool,
    ) -> Result<(usize, String), ReadError> {
        let mut searched = 0;
        let mut last_activity = Instant::now();
        let mut last_cpu = if watch_idle { self.cpu_seconds() } else { None };
        let mut last_memory_check = Instant::now();
        loop {
            if let Some((which, before, after)) = find_line(&self.out, markers, searched) {
                let text = self.out[..before].to_string();
                self.out.drain(..after);
                return Ok((which, text));
            }
            // where to search next: the last line may still grow
            searched = self.out.rfind('\n').map(|i| i + 1).unwrap_or(0);
            if self.out.len() > MAX_OUTPUT {
                return Err(ReadError::TooMuch);
            }
            if self.closed >= 2 {
                let how = self.exit_text();
                return Err(ReadError::Ended(format!(
                    "{how}{}",
                    tail(&format!("{}{}", self.out, self.err))
                )));
            }
            let now = Instant::now();
            if let Some(limit) = self.memory_limit
                && now.duration_since(last_memory_check) >= Duration::from_millis(250)
            {
                last_memory_check = now;
                if crate::procinfo::group_memory_bytes(self.pgid).is_some_and(|used| used > limit) {
                    return Err(ReadError::Memory(limit));
                }
            }
            if now >= deadline {
                return Err(ReadError::Timeout);
            }
            let wait = (deadline - now).min(Duration::from_millis(250));
            match self.rx.recv_timeout(wait) {
                Ok(ev) => {
                    self.take(ev);
                    self.drain();
                    last_activity = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {
                    if watch_idle && last_activity.elapsed() >= self.idle {
                        let cpu = self.cpu_seconds();
                        match (last_cpu, cpu) {
                            (Some(a), Some(b)) if b - a < 0.02 => {
                                return Err(ReadError::Waiting(tail(&format!(
                                    "{}{}",
                                    self.out, self.err
                                ))));
                            }
                            _ => {}
                        }
                        last_cpu = cpu;
                        last_activity = Instant::now();
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.closed = 2;
                }
            }
        }
    }

    /// How the system's main process ended, as `(exit status 3) ` or
    /// `(killed by signal 9) `; empty where it is not known.
    fn exit_text(&mut self) -> String {
        use std::os::unix::process::ExitStatusExt;
        // the output closes a moment before the process can be waited for
        for _ in 0..20 {
            if let Ok(Some(st)) = self.child.try_wait() {
                return match (st.code(), st.signal()) {
                    (Some(c), _) => format!("(exit status {c}) "),
                    (None, Some(9)) => {
                        "(killed by signal 9, as by the kernel when memory runs out) ".into()
                    }
                    (None, Some(sig)) => format!("(killed by signal {sig}) "),
                    _ => String::new(),
                };
            }
            thread::sleep(Duration::from_millis(10));
        }
        String::new()
    }

    /// The CPU seconds of every process of the system's group, as the
    /// operating system counts them.
    pub fn cpu_seconds(&self) -> Option<f64> {
        crate::procinfo::group_cpu_seconds(self.pgid)
    }

    /// Kill the whole process group, and wait for the system.
    pub fn kill(&mut self) {
        self.stdin = None;
        unregister(self.pgid);
        unsafe {
            libc::killpg(self.pgid, libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// End the system: close its input, give it a moment, then kill it.
    pub fn close(&mut self, goodbye: &str) {
        if !goodbye.is_empty() {
            let _ = self.send(goodbye);
        }
        self.stdin = None;
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        self.kill();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            self.kill();
        } else {
            unsafe {
                libc::killpg(self.pgid, libc::SIGKILL);
            }
        }
    }
}

/// Find a line that is one of `markers` alone, from `from` on: which, the
/// byte where the line starts and the byte after its end.
fn find_line(text: &str, markers: &[&str], from: usize) -> Option<(usize, usize, usize)> {
    let mut start = from;
    // begin at a line start
    if start > 0 && !text[..start].ends_with('\n') {
        start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    }
    let mut pos = start;
    while pos <= text.len() {
        let end = text[pos..].find('\n').map(|i| pos + i);
        let Some(end) = end else { return None };
        let line = text[pos..end].trim_end_matches('\r').trim();
        if let Some(which) = markers.iter().position(|m| *m == line) {
            return Some((which, pos, end + 1));
        }
        pos = end + 1;
    }
    None
}

/// Why a program cannot be started, and what to do about it.
fn spawn_message(program: &str, e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => format!(
            "cannot start `{program}`: no such program on the PATH. Install the system, or give \
             the program's full path with --command"
        ),
        std::io::ErrorKind::PermissionDenied => format!(
            "cannot start `{program}`: permission denied. Is it the system's command-line \
             program? (--command gives another)"
        ),
        _ => format!("cannot start `{program}`: {e}"),
    }
}

/// The last few lines of a text, for a message.
pub fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let n = lines.len();
    let s = lines[n.saturating_sub(6)..].join("\n");
    cut(&s, 2000)
}

pub fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_alone_on_a_line() {
        let t = "a\n x @@END 1\n@@END 1\nrest";
        assert_eq!(find_line(t, &["@@END 1"], 0), Some((0, 13, 21)));
        assert_eq!(
            find_line("@@END 1\n", &["x", "@@END 1"], 0),
            Some((1, 0, 8))
        );
        assert_eq!(find_line("@@END 1", &["@@END 1"], 0), None);
    }

    #[test]
    fn a_process_round_trip_and_timeout() {
        let spec = Spec {
            program: "/bin/sh".into(),
            args: vec![],
            env: vec![],
            dir: None,
        };
        let mut s = Session::spawn(&spec).unwrap();
        s.send("echo hello; echo @@DONE").unwrap();
        let got = s
            .read_to("@@DONE", Instant::now() + Duration::from_secs(5), false)
            .unwrap();
        assert_eq!(got, "hello\n");
        s.send("sleep 30; echo @@DONE").unwrap();
        let r = s.read_to("@@DONE", Instant::now() + Duration::from_millis(300), false);
        assert!(matches!(r, Err(ReadError::Timeout)));
        s.kill();
    }

    fn shell() -> Session {
        Session::spawn(&Spec {
            program: "/bin/sh".into(),
            args: vec![],
            env: vec![],
            dir: None,
        })
        .unwrap()
    }

    #[test]
    fn a_crash_says_how_the_system_ended() {
        let mut s = shell();
        s.send("echo before; kill -9 $$").unwrap();
        match s.read_to("@@DONE", Instant::now() + Duration::from_secs(10), false) {
            Err(ReadError::Ended(t)) => {
                assert!(t.contains("killed by signal 9"), "{t}");
                assert!(t.contains("before"), "{t}");
            }
            other => panic!("{other:?}"),
        }
        let mut s = shell();
        s.send("exit 3").unwrap();
        match s.read_to("@@DONE", Instant::now() + Duration::from_secs(10), false) {
            Err(ReadError::Ended(t)) => assert!(t.contains("exit status 3"), "{t}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn too_much_memory_stops_the_system() {
        let mut s = shell();
        if crate::procinfo::group_memory_bytes(s.pgid).is_none() {
            return;
        }
        // a limit no process meets
        s.set_memory_limit(Some(1000));
        s.send("sleep 20; echo @@DONE").unwrap();
        let t0 = Instant::now();
        let r = s.read_to("@@DONE", Instant::now() + Duration::from_secs(15), false);
        assert!(matches!(r, Err(ReadError::Memory(1000))), "{r:?}");
        assert!(t0.elapsed() < Duration::from_secs(5));
        s.kill();
        // and one that no process reaches
        let mut s = shell();
        s.set_memory_limit(Some(1 << 40));
        s.send("echo hi; echo @@DONE").unwrap();
        let r = s.read_to("@@DONE", Instant::now() + Duration::from_secs(10), false);
        assert_eq!(r.unwrap(), "hi\n");
    }

    #[test]
    fn a_missing_program_says_what_to_do() {
        let spec = Spec {
            program: "no-such-cas-program".into(),
            args: vec![],
            env: vec![],
            dir: None,
        };
        let e = Session::spawn(&spec).err().unwrap().to_string();
        assert!(
            e.contains("`no-such-cas-program`") && e.contains("--command"),
            "{e}"
        );
    }

    #[test]
    fn waiting_for_input_is_seen() {
        let spec = Spec {
            program: "/bin/sh".into(),
            args: vec![],
            env: vec![],
            dir: None,
        };
        let mut s = Session::spawn(&spec).unwrap();
        if s.cpu_seconds().is_none() {
            return;
        }
        s.send("echo 'Is a positive?'; read answer; echo @@DONE")
            .unwrap();
        let r = s.read_to("@@DONE", Instant::now() + Duration::from_secs(20), true);
        match r {
            Err(ReadError::Waiting(t)) => assert!(t.contains("Is a positive?")),
            other => panic!("{other:?}"),
        }
    }
}

/// The process groups of the running systems, which a signal to the runner
/// kills: the systems run in groups of their own, so that the terminal's
/// Ctrl-C does not reach them.
static GROUPS: [std::sync::atomic::AtomicI32; 256] =
    [const { std::sync::atomic::AtomicI32::new(0) }; 256];

fn register(pgid: i32) {
    use std::sync::atomic::Ordering;
    for slot in &GROUPS {
        if slot
            .compare_exchange(0, pgid, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return;
        }
    }
}

fn unregister(pgid: i32) {
    use std::sync::atomic::Ordering;
    for slot in &GROUPS {
        let _ = slot.compare_exchange(pgid, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

extern "C" fn on_signal(sig: libc::c_int) {
    use std::sync::atomic::Ordering;
    for slot in &GROUPS {
        let g = slot.load(Ordering::SeqCst);
        if g > 0 {
            unsafe { libc::killpg(g, libc::SIGKILL) };
        }
    }
    unsafe { libc::_exit(128 + sig) };
}

/// Kill the systems when the runner is interrupted or terminated.
pub fn install_signal_handlers() {
    unsafe {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(sig, on_signal as *const () as libc::sighandler_t);
        }
    }
}
