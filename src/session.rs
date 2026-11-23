//! A system's process: started once and kept, fed the problems through its
//! standard input, read through its standard output, killed and started
//! again where a problem runs out of time.
//!
//! The system runs in a process group of its own (on Windows a job object),
//! so that the runner kills every process it starts (as Maple's kernel, or
//! FriCAS's Lisp). Its CPU time is read from the operating system for the
//! whole group, for a problem that runs out of time, and to tell a system
//! that waits for input (as Maxima asking "Is a positive?") from one that
//! computes.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

use crate::procinfo::Group;

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
    group: Group,
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
            .stderr(Stdio::piped());
        Group::prepare(&mut cmd);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        // A runner that dies (kill -9, a crash) must not leave the system
        // computing on: on Linux the kernel kills it with the runner.
        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        if let Some(dir) = &spec.dir {
            cmd.current_dir(dir);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| std::io::Error::new(e.kind(), spawn_message(&spec.program, &e)))?;
        let group = match Group::of(&child) {
            Ok(g) => g,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    e.kind(),
                    format!("cannot start `{}`: {e}", spec.program),
                ));
            }
        };
        #[cfg(unix)]
        register(group.pgid);
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
            group,
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
                if crate::procinfo::group_memory_bytes(&self.group).is_some_and(|used| used > limit)
                {
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
        // the output closes a moment before the process can be waited for
        for _ in 0..20 {
            if let Ok(Some(st)) = self.child.try_wait() {
                return exit_status_text(st);
            }
            thread::sleep(Duration::from_millis(10));
        }
        String::new()
    }

    /// The CPU seconds of every process of the system's group, as the
    /// operating system counts them.
    pub fn cpu_seconds(&self) -> Option<f64> {
        crate::procinfo::group_cpu_seconds(&self.group)
    }

    /// Kill the whole process group, and wait for the system.
    pub fn kill(&mut self) {
        self.stdin = None;
        #[cfg(unix)]
        unregister(self.group.pgid);
        self.group.kill();
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
            self.group.kill();
        }
    }
}

/// How a process ended, for a message: `(exit status 3) `, on Unix
/// `(killed by signal 9) `, on Windows an NTSTATUS in hex, with what the
/// common ones mean.
#[cfg(unix)]
fn exit_status_text(st: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (st.code(), st.signal()) {
        (Some(c), _) => format!("(exit status {c}) "),
        (None, Some(9)) => "(killed by signal 9, as by the kernel when memory runs out) ".into(),
        (None, Some(sig)) => format!("(killed by signal {sig}) "),
        _ => String::new(),
    }
}

#[cfg(windows)]
fn exit_status_text(st: std::process::ExitStatus) -> String {
    match st.code() {
        Some(c) => windows_exit_text(c),
        None => String::new(),
    }
}

/// A Windows exit code: an NTSTATUS (0xC...) where the process crashed.
#[cfg(any(windows, test))]
fn windows_exit_text(code: i32) -> String {
    let u = code as u32;
    if u < 0xC000_0000 {
        return format!("(exit status {code}) ");
    }
    let what = match u {
        0xC000_0005 => ": an access violation",
        0xC000_0017 | 0xC000_012D => ": out of memory",
        0xC000_00FD => ": a stack overflow",
        0xC000_0409 => ": a fail-fast exit",
        0xC000_013A => ": Ctrl-C",
        _ => "",
    };
    format!("(exit status 0x{u:08X}{what}) ")
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    fn shell() -> Session {
        Session::spawn(&Spec {
            program: "/bin/sh".into(),
            args: vec![],
            env: vec![],
            dir: None,
        })
        .unwrap()
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn too_much_memory_stops_the_system() {
        let mut s = shell();
        if crate::procinfo::group_memory_bytes(&s.group).is_none() {
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

    #[cfg(unix)]
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

    #[test]
    fn windows_exit_codes_say_what_they_mean() {
        assert_eq!(windows_exit_text(3), "(exit status 3) ");
        assert_eq!(
            windows_exit_text(0xC000_0005u32 as i32),
            "(exit status 0xC0000005: an access violation) "
        );
        assert_eq!(
            windows_exit_text(0xC000_0123u32 as i32),
            "(exit status 0xC0000123) "
        );
    }
}

/// The same, on Windows: cmd.exe as the system. Its prompt is set to a line
/// break, so that what it prints begins a line.
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    fn cmd(args: &[&str]) -> Spec {
        Spec {
            program: "cmd.exe".into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: vec![("PROMPT".into(), "$_".into())],
            dir: None,
        }
    }

    #[test]
    fn a_process_round_trip_and_timeout() {
        let mut s = Session::spawn(&cmd(&["/Q", "/D"])).unwrap();
        s.send("echo hello& echo @@DONE").unwrap();
        let got = s
            .read_to("@@DONE", Instant::now() + Duration::from_secs(10), false)
            .unwrap();
        assert!(got.ends_with("hello\r\n"), "{got:?}");
        s.send("ping -n 30 127.0.0.1 >nul& echo @@DONE").unwrap();
        let r = s.read_to("@@DONE", Instant::now() + Duration::from_millis(500), false);
        assert!(matches!(r, Err(ReadError::Timeout)), "{r:?}");
        s.kill();
    }

    #[test]
    fn an_exit_says_how_the_system_ended() {
        let mut s = Session::spawn(&cmd(&["/D", "/C", "echo before& exit 3"])).unwrap();
        match s.read_to("@@DONE", Instant::now() + Duration::from_secs(10), false) {
            Err(ReadError::Ended(t)) => {
                assert!(t.contains("exit status 3"), "{t}");
                assert!(t.contains("before"), "{t}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// The processes the system started are killed with it: ping, started by
    /// cmd, holds the output pipes, which close only once it has ended too.
    #[test]
    fn the_job_kills_every_process_of_the_system() {
        let mut s = Session::spawn(&cmd(&["/D", "/C", "ping -n 60 127.0.0.1"])).unwrap();
        thread::sleep(Duration::from_millis(500));
        assert!(s.cpu_seconds().is_some());
        s.group.kill();
        let until = Instant::now() + Duration::from_secs(10);
        while s.closed < 2 && Instant::now() < until {
            if let Ok(ev) = s.rx.recv_timeout(Duration::from_millis(100)) {
                s.take(ev);
            }
        }
        assert_eq!(s.closed, 2, "the output stays open: a process lives on");
    }

    #[test]
    fn too_much_memory_stops_the_system() {
        let mut s = Session::spawn(&cmd(&["/Q", "/D"])).unwrap();
        assert!(crate::procinfo::group_memory_bytes(&s.group).is_some_and(|m| m > 0));
        s.set_memory_limit(Some(1000));
        s.send("ping -n 20 127.0.0.1 >nul& echo @@DONE").unwrap();
        let t0 = Instant::now();
        let r = s.read_to("@@DONE", Instant::now() + Duration::from_secs(15), false);
        assert!(matches!(r, Err(ReadError::Memory(1000))), "{r:?}");
        assert!(t0.elapsed() < Duration::from_secs(5));
        s.kill();
        let mut s = Session::spawn(&cmd(&["/Q", "/D"])).unwrap();
        s.set_memory_limit(Some(1 << 40));
        s.send("echo hi& echo @@DONE").unwrap();
        let r = s.read_to("@@DONE", Instant::now() + Duration::from_secs(10), false);
        assert!(r.unwrap().ends_with("hi\r\n"));
    }

    #[test]
    fn waiting_for_input_is_seen() {
        let mut s = Session::spawn(&cmd(&["/Q", "/D"])).unwrap();
        s.send("set /p answer=Is a positive?").unwrap();
        let r = s.read_to("@@DONE", Instant::now() + Duration::from_secs(20), true);
        match r {
            Err(ReadError::Waiting(t)) => assert!(t.contains("Is a positive?"), "{t}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cpu_time_is_counted() {
        let mut s = Session::spawn(&cmd(&["/Q", "/D"])).unwrap();
        let c0 = s.cpu_seconds().unwrap();
        // a busy loop of cmd's own
        s.send("(for /l %i in (1,1,50000) do @set x=%i)& echo @@DONE")
            .unwrap();
        s.read_to("@@DONE", Instant::now() + Duration::from_secs(60), false)
            .unwrap();
        let c1 = s.cpu_seconds().unwrap();
        assert!(c1 > c0, "{c0} {c1}");
    }
}

/// The process groups of the running systems, which a signal to the runner
/// kills: the systems run in groups of their own, so that the terminal's
/// Ctrl-C does not reach them.
#[cfg(unix)]
static GROUPS: [std::sync::atomic::AtomicI32; 256] =
    [const { std::sync::atomic::AtomicI32::new(0) }; 256];

#[cfg(unix)]
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

#[cfg(unix)]
fn unregister(pgid: i32) {
    use std::sync::atomic::Ordering;
    for slot in &GROUPS {
        let _ = slot.compare_exchange(pgid, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

#[cfg(unix)]
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
#[cfg(unix)]
pub fn install_signal_handlers() {
    unsafe {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(sig, on_signal as *const () as libc::sighandler_t);
        }
    }
}

/// On Windows nothing is needed: each system is in a job object that is
/// killed when the runner ends, however it ends (Ctrl-C, closed, killed).
#[cfg(windows)]
pub fn install_signal_handlers() {}
