//! Nothing private goes in the upload: a system's error message may carry the
//! home directory, the working directory, the user name or the host name (a
//! traceback's file paths, a prompt), and the messages are written to the
//! run file, which is uploaded. They are scrubbed as they are written.

/// What is private here: the paths and names that a message must not carry.
pub struct Private {
    /// Paths, replaced as a whole: (path, what stands for it).
    paths: Vec<(String, &'static str)>,
    /// Names, replaced where they stand as a word of their own.
    names: Vec<(String, &'static str)>,
}

impl Private {
    /// The home and working directories, the user's and the host's names of
    /// this process.
    pub fn here() -> Private {
        let mut paths = Vec::new();
        if let Ok(h) = std::env::var("HOME") {
            paths.push((h, "~"));
        }
        if let Ok(c) = std::env::current_dir() {
            paths.push((c.display().to_string(), "."));
        }
        let mut names = Vec::new();
        for var in ["USER", "LOGNAME"] {
            if let Ok(u) = std::env::var(var) {
                names.push((u, "<user>"));
            }
        }
        if let Some(h) = hostname() {
            names.push((h.clone(), "<host>"));
            if let Some((short, _)) = h.split_once('.') {
                names.push((short.to_string(), "<host>"));
            }
        }
        Private::new(paths, names)
    }

    pub fn new(
        mut paths: Vec<(String, &'static str)>,
        mut names: Vec<(String, &'static str)>,
    ) -> Private {
        // "/" and the like are no secret; short names would eat words
        paths.retain(|(p, _)| p.trim_end_matches('/').len() > 1);
        names.retain(|(n, _)| n.len() >= 3);
        // the longest first, so that /home/me/x is not cut at /home/me
        paths.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        names.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        names.dedup();
        Private { paths, names }
    }

    pub fn scrub(&self, text: &str) -> String {
        let mut s = text.to_string();
        for (p, to) in &self.paths {
            let p = p.trim_end_matches('/');
            s = replace_words(&s, p, to, |before, after| {
                // not the start or the end of a longer name
                !is_name_char(before) && !after.is_some_and(is_name_char)
            });
        }
        for (n, to) in &self.names {
            s = replace_words(&s, n, to, |before, after| {
                !is_name_char(before) && !after.is_some_and(is_name_char)
            });
        }
        s
    }
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Replace each `what` in `s` by `to`, where `ok(char before, char after)`.
fn replace_words(s: &str, what: &str, to: &str, ok: impl Fn(char, Option<char>) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    let mut before = ' ';
    while let Some(i) = rest.find(what) {
        let head = &rest[..i];
        let b = head.chars().next_back().unwrap_or(before);
        let tail = &rest[i + what.len()..];
        out.push_str(head);
        if ok(b, tail.chars().next()) {
            out.push_str(to);
        } else {
            out.push_str(what);
        }
        before = what.chars().next_back().unwrap_or(' ');
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn hostname() -> Option<String> {
    let mut buf = [0 as libc::c_char; 256];
    if unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len() - 1) } != 0 {
        return None;
    }
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    let h = String::from_utf8_lossy(&bytes).to_string();
    (!h.is_empty()).then_some(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private() -> Private {
        Private::new(
            vec![
                ("/home/anna".into(), "~"),
                ("/home/anna/work/run".into(), "."),
                ("/".into(), "ROOT"),
            ],
            vec![
                ("anna".into(), "<user>"),
                ("bowser".into(), "<host>"),
                ("ab".into(), "X"),
            ],
        )
    }

    #[test]
    fn paths_names_and_hosts_are_replaced() {
        let p = private();
        assert_eq!(
            p.scrub("File \"/home/anna/venv/lib/x.py\", line 3\nbowser: anna@bowser"),
            "File \"~/venv/lib/x.py\", line 3\n<host>: <user>@<host>"
        );
        // the longer path wins
        assert_eq!(p.scrub("/home/anna/work/run/x"), "./x");
    }

    #[test]
    fn words_that_only_contain_a_name_stay() {
        let p = private();
        assert_eq!(
            p.scrub("annamaria, banna, anna-lise"),
            "annamaria, banna, anna-lise"
        );
        assert_eq!(p.scrub("ab / cd and no secrets"), "ab / cd and no secrets");
        assert_eq!(p.scrub("/home/annabelle/x"), "/home/annabelle/x");
    }
}
