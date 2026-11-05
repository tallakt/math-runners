//! Mathilda (github.com/stblake/mathilda), Sam Blake's open-source system in
//! the Wolfram Language: the problems as for Mathematica, sent to one session
//! of `Mathilda` from a pipe.
//!
//! Written from its user documentation and the Wolfram Language alone, not
//! from its source; untested until it runs on the reference server.

use super::Adapter;
use super::mathematica::{problem_text, start_text};
use crate::print::Unsupported;
use crate::problem::Problem;
use crate::session::Spec;

pub struct Mathilda;

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
    }

    fn problem(&self, p: &Problem, nonce: &str) -> Result<String, Unsupported> {
        problem_text(p, nonce)
    }

    fn goodbye(&self) -> &'static str {
        "Quit[]"
    }

    fn tested(&self) -> bool {
        false
    }
}
