//! The round trips of the adapters' printers for nested forms: a form printed in the system's
//! syntax, given to the system, its value read back and compared. A test on a machine without the
//! system says so and passes: the systems are not in the build's environment.

use std::io::Write;
use std::process::{Command, Stdio};

/// The forms of the formats' section 3 as neutral text, and the value of each.
pub const CASES: &[(&str, &str)] = &[
    ("integrate(x^2, x, 0, 3)", "9"),
    ("sum(k, k, 1, 4)", "10"),
    ("product(k, k, 1, 4)", "24"),
    ("limit(sin(x)/x, x, 0)", "1"),
    // a form inside a form
    ("sum(integrate(x^3, x, 0, k), k, 1, 3)", "49/2"),
];

/// Forms whose printing only is tested (the systems write their values differently): a limit at
/// infinity, an integral over the line.
pub const PRINT_ONLY: &[&str] = &[
    "limit((1 + 1/n)^n, n, infinity)",
    "integrate(exp(-x^2), x, -infinity, infinity)",
];

/// What the program prints on its standard output given `stdin`; none where it is not on the PATH
/// or does not end well.
pub fn run(program: &str, args: &[&str], stdin: &str) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(stdin.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Compares the values read with the cases', with the system's name for the failure.
pub fn check(system: &str, got: &[String]) {
    assert_eq!(got.len(), CASES.len(), "{system}: {got:?}");
    for ((neutral, want), got) in CASES.iter().zip(got) {
        assert_eq!(got, want, "{system}: {neutral}");
    }
}

/// Says a test is skipped, where the system is not here.
pub fn skipped(system: &str) {
    eprintln!("nested forms round trip skipped: {system} is not installed here");
}
