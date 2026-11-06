//! Mathilda (github.com/stblake/mathilda), Sam Blake's open-source system in
//! the Wolfram Language: the problems as for Mathematica, sent to one session
//! of `Mathilda` from a pipe.
//!
//! With its stdin a pipe, Mathilda reads one request a line, `{"id": ..,
//! "expr": ".."}` (a line without its `id` goes unanswered), and answers each with JSON lines (`"type": "expr"`,
//! `"error"`, `"done"`), while `WriteString["stdout", ..]` prints as it is:
//! the protocol's lines are printed that way, and the JSON is left out of the
//! messages. The timed task, its time and its answer are one request, so
//! that a task Mathilda cannot read gives no answer but its parse error.
//!
//! Written from its user documentation and its behaviour, not from its source.

use super::Adapter;
use super::mathematica::{problem_parts, start_text};
use crate::print::Unsupported;
use crate::problem::Problem;
use crate::session::Spec;
use serde_json::Value;

pub struct Mathilda;

/// One request of the pipe protocol: without an `id`, Mathilda leaves it
/// unanswered.
fn request(expr: &str) -> String {
    serde_json::json!({ "id": 1, "expr": expr }).to_string()
}

/// A line of the protocol's JSON, as the message it carries, if any; None
/// where the line is not the protocol's.
fn protocol_line(line: &str) -> Option<Option<String>> {
    let t = line.trim();
    if !t.starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(t).ok()?;
    match v.get("type")?.as_str()? {
        "error" => Some(Some(
            v.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("an error")
                .to_string(),
        )),
        _ => Some(None),
    }
}

impl Adapter for Mathilda {
    fn name(&self) -> &'static str {
        "Mathilda"
    }

    fn slug(&self) -> &'static str {
        "mathilda"
    }

    fn default_command(&self) -> &'static str {
        "Mathilda"
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: program.to_string(),
            args: vec![],
            env: vec![],
            dir: None,
        }
    }

    fn start(&self, nonce: &str) -> String {
        start_text(nonce)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(request)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        let t = problem_parts(p, nonce)?;
        Ok([
            request(&t.reset),
            request(&t.begin),
            request(&t.work.join(" ")),
            request(&t.end),
        ]
        .join("\n"))
    }

    fn goodbye(&self) -> &'static str {
        r#"{"id": 1, "expr": "Quit[]"}"#
    }

    fn clean_noise(&self, text: &str) -> String {
        text.lines()
            .filter_map(|l| match protocol_line(l) {
                None => Some(l.to_string()),
                Some(message) => message,
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }

    /// `Mathilda 0.6222 (GCC 14.2.0, GMP 6.3.0, ...)`: the version only,
    /// `0.6222`, without the name and the libraries it was built with.
    fn clean_version(&self, text: &str) -> String {
        let t = text.trim();
        let t = t.strip_prefix("Mathilda ").unwrap_or(t);
        t.split_whitespace().next().unwrap_or(t).to_string()
    }

    fn tested(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_noise() {
        assert_eq!(request("1 + \"a\""), r#"{"id":1,"expr":"1 + \"a\""}"#);
        let noise = "{\"id\":1,\"type\":\"expr\",\"payload\":\"2\"}\n\
                     {\"type\":\"error\",\"message\":\"Parse error: Integrate[\"}\n\
                     {\"id\":1,\"type\":\"done\",\"memory\":1}\n\
                     a warning";
        assert_eq!(
            Mathilda.clean_noise(noise),
            "Parse error: Integrate[\na warning"
        );
        assert_eq!(
            Mathilda.clean_version("Mathilda 0.6222 (GCC 14.2.0, GMP 6.3.0)"),
            "0.6222"
        );
    }
}
