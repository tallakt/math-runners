//! FriCAS (to be written).

use super::Adapter;
use crate::print::{Unsupported, unsupported};
use crate::problem::Problem;
use crate::session::Spec;

pub struct FriCAS;

impl Adapter for FriCAS {
    fn name(&self) -> &'static str {
        "FriCAS"
    }

    fn slug(&self) -> &'static str {
        "fricas"
    }

    fn default_command(&self) -> &'static str {
        "fricas"
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
