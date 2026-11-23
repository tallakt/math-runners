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
        // Windows: the profile is the home; HOME is set by some shells
        for var in ["HOME", "USERPROFILE"] {
            if let Ok(h) = std::env::var(var) {
                for p in spellings(&h) {
                    paths.push((p, "~"));
                }
            }
        }
        if let Ok(c) = std::env::current_dir() {
            for p in spellings(&c.display().to_string()) {
                paths.push((p, "."));
            }
        }
        let mut names = Vec::new();
        for var in ["USER", "LOGNAME", "USERNAME"] {
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
        // "/", "C:\" and the like are no secret; short names would eat words
        paths.retain(|(p, _)| !trivial_path(p));
        names.retain(|(n, _)| n.len() >= 3);
        // the longest first, so that /home/me/x is not cut at /home/me
        paths.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        paths.dedup();
        names.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        names.dedup();
        Private { paths, names }
    }

    pub fn scrub(&self, text: &str) -> String {
        let mut s = text.to_string();
        for (p, to) in &self.paths {
            let p = p.trim_end_matches(['/', '\\']);
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

/// Windows's paths and names are matched in any case (`C:\Users\Anna` is
/// `c:\users\anna`); Unix's as they are.
const IGNORE_CASE: bool = cfg!(windows);

/// A path that is no secret: the root, or a drive's (`C:`, `C:\`).
fn trivial_path(p: &str) -> bool {
    let p = p.trim_end_matches(['/', '\\']);
    let b = p.as_bytes();
    p.len() <= 1 || (b.len() == 2 && b[1] == b':' && b[0].is_ascii_alphabetic())
}

/// The ways a path may be written in a message: as it is and, on Windows,
/// with `/` or doubled `\` for `\`, and its short (8.3) form, as
/// `C:\Users\ANNAMA~1`, which temporary paths have.
fn spellings(path: &str) -> Vec<String> {
    let mut out = vec![path.to_string()];
    if cfg!(windows) {
        if let Some(short) = short_path(path) {
            out.push(short);
        }
        for p in out.clone() {
            out.push(p.replace('\\', "/"));
            out.push(p.replace('\\', "\\\\"));
        }
    }
    out.dedup();
    out
}

#[cfg(windows)]
fn short_path(path: &str) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let long: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetShortPathNameW(long.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 || n as usize >= buf.len() {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..n as usize]);
    (!s.eq_ignore_ascii_case(path)).then_some(s)
}

#[cfg(not(windows))]
fn short_path(_path: &str) -> Option<String> {
    None
}

/// Replace each `what` in `s` by `to`, where `ok(char before, char after)`.
fn replace_words(s: &str, what: &str, to: &str, ok: impl Fn(char, Option<char>) -> bool) -> String {
    replace_words_case(s, what, to, IGNORE_CASE, ok)
}

fn replace_words_case(
    s: &str,
    what: &str,
    to: &str,
    ignore_case: bool,
    ok: impl Fn(char, Option<char>) -> bool,
) -> String {
    if what.is_empty() {
        return s.to_string();
    }
    // ASCII case folding keeps every byte where it was: the places found in
    // the folded text are those of the text
    let (hay, needle) = if ignore_case {
        (s.to_ascii_lowercase(), what.to_ascii_lowercase())
    } else {
        (s.to_string(), what.to_string())
    };
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    let mut before = ' ';
    while let Some(i) = hay[pos..].find(&needle) {
        let (start, end) = (pos + i, pos + i + needle.len());
        let head = &s[pos..start];
        let b = head.chars().next_back().unwrap_or(before);
        out.push_str(head);
        if ok(b, s[end..].chars().next()) {
            out.push_str(to);
        } else {
            out.push_str(&s[start..end]);
        }
        before = s[start..end].chars().next_back().unwrap_or(' ');
        pos = end;
    }
    out.push_str(&s[pos..]);
    out
}

#[cfg(windows)]
fn hostname() -> Option<String> {
    let h = std::env::var("COMPUTERNAME").ok()?;
    (!h.is_empty()).then_some(h)
}

#[cfg(unix)]
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
    fn windows_paths_in_any_case_and_spelling() {
        let p = Private::new(
            vec![
                (r"C:\Users\Anna".into(), "~"),
                ("C:/Users/Anna".into(), "~"),
                (r"C:\Users\ANNA~1".into(), "~"),
                (r"C:\".into(), "ROOT"),
            ],
            vec![("Anna".into(), "<user>")],
        );
        let s = |t: &str, ci: bool| {
            let mut out = t.to_string();
            for (path, to) in &p.paths {
                let path = path.trim_end_matches(['/', '\\']);
                out = replace_words_case(&out, path, to, ci, |b, a| {
                    !is_name_char(b) && !a.is_some_and(is_name_char)
                });
            }
            out
        };
        assert_eq!(
            s(
                r"Get[c:\users\anna\x.m] C:/Users/Anna/y C:\Users\ANNA~1\Temp",
                true
            ),
            r"Get[~\x.m] ~/y ~\Temp"
        );
        // as they are, where case counts
        assert_eq!(s(r"c:\users\anna\x", false), r"c:\users\anna\x");
        // not a longer name, not the drive
        assert_eq!(
            s(r"C:\Users\Annabel\x C:\x", true),
            r"C:\Users\Annabel\x C:\x"
        );
        assert!(trivial_path(r"C:\") && trivial_path("D:") && trivial_path("/"));
        assert!(!trivial_path(r"C:\x"));
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
