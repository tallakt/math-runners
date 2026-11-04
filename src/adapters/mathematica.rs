//! Mathematica (to be written).

use super::Adapter;
use crate::print::{Unsupported, unsupported};
use crate::problem::Problem;
use crate::session::Spec;

pub struct Mathematica;

impl Adapter for Mathematica {
    fn name(&self) -> &'static str {
        "Mathematica"
    }

    fn slug(&self) -> &'static str {
        "mathematica"
    }

    fn default_command(&self) -> &'static str {
        "wolfram"
    }

    fn spec(&self, program: &str) -> Spec {
        Spec {
            program: program.to_string(),
            args: vec![],
            env: vec![],
            dir: None,
        }
    }

    fn start(&self, _nonce: &str) -> String {
        String::new()
    }

    fn problem(&self, _p: &Problem, _nonce: &str) -> Result<String, Unsupported> {
        unsupported("not written")
    }

    fn goodbye(&self) -> &'static str {
        ""
    }
}
