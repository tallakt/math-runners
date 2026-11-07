//! The site's API (section 6 of the formats): the suites, and the upload.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// The production site, the default of `--site`.
pub const SITE: &str = "https://math.osp.no";

/// Names under `localhost` (RFC 6761) are the machine itself, as
/// `math.localhost`, the site's copy in development, which the system's
/// resolver may not know.
#[derive(Debug, Default)]
struct Resolver(DefaultResolver);

impl ureq::unversioned::resolver::Resolver for Resolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let host = uri.host().unwrap_or("");
        if host == "localhost" || host.ends_with(".localhost") {
            let https = uri.scheme_str() == Some("https");
            let port = uri.port_u16().unwrap_or(if https { 443 } else { 80 });
            let mut addrs = self.empty();
            addrs.push(std::net::SocketAddr::from(([127, 0, 0, 1], port)));
            return Ok(addrs);
        }
        self.0.resolve(uri, config, timeout)
    }
}

/// The seconds a small request (the suites' list, a status) has, in all.
const API_TIMEOUT: u64 = 60;
/// The seconds the suite's download has, in all.
const DOWNLOAD_TIMEOUT: u64 = 1800;
/// The seconds an upload has, in all.
const UPLOAD_TIMEOUT: u64 = 600;

/// An agent that gives up: without a limit a server that stops answering
/// holds the runner for ever.
fn agent(seconds: u64) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent(concat!("math-run/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(seconds)))
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::default(), Resolver::default())
}

/// Whether a request failed before anything reached the site.
fn before_the_site(e: &ureq::Error) -> bool {
    matches!(e, ureq::Error::HostNotFound | ureq::Error::ConnectionFailed)
        || matches!(e, ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused)
}

/// What went wrong with a request, in words.
fn explain(url: &str, e: &ureq::Error) -> String {
    let site = url.split('/').take(3).collect::<Vec<_>>().join("/");
    let why = match e {
        ureq::Error::HostNotFound => {
            format!("the host of {site} was not found (is there a network? is --site right?)")
        }
        ureq::Error::ConnectionFailed => format!("{site} did not accept the connection"),
        ureq::Error::Timeout(_) => format!("{site} did not answer in time"),
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => {
            format!("{site} refused the connection")
        }
        other => other.to_string(),
    };
    format!("cannot reach {url}: {why}")
}

pub fn get_json(url: &str) -> Result<Value, String> {
    let mut r = agent(API_TIMEOUT).get(url).call().map_err(|e| {
        format!(
            "{}. Fetching a suite and uploading need the network; a run on a local suite \
             file (--suite FILE) does not",
            explain(url, &e)
        )
    })?;
    let status = r.status().as_u16();
    let body = r
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("{url}: {e}"))?;
    if status != 200 {
        return Err(format!(
            "{url}: {status} {}",
            crate::session::cut(&body, 300)
        ));
    }
    serde_json::from_str(&body)
        .map_err(|e| format!("{url}: the answer is not JSON ({e}): is --site a math.osp.no?"))
}

pub fn download(url: &str, to: &Path) -> Result<(), String> {
    let mut r = agent(DOWNLOAD_TIMEOUT)
        .get(url)
        .call()
        .map_err(|e| explain(url, &e))?;
    let status = r.status().as_u16();
    if status != 200 {
        return Err(format!("{url}: {status}"));
    }
    let mut f = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut body = r.body_mut().with_config().limit(u64::MAX).reader();
    std::io::copy(&mut body, &mut f).map_err(|e| format!("{url}: the download broke off: {e}"))?;
    f.flush().map_err(|e| e.to_string())
}

/// The token as the site takes it: one word of printable characters.
fn clean_token(token: Option<&str>, site: &str) -> Result<String, String> {
    let site = site.trim_end_matches('/');
    let t = token.map(str::trim).unwrap_or("");
    if t.is_empty() {
        return Err(format!(
            "no upload token. Register at {site}/register (the link mailed to you shows the token, \
             and {site}/account shows it again, or a new one), then give it as the environment \
             variable MATH_OSP_TOKEN (`export MATH_OSP_TOKEN=...`) or with --token"
        ));
    }
    if !t.chars().all(|c| c.is_ascii_graphic()) {
        return Err(
            "the token has characters a token does not (spaces, line breaks): copy it again from \
             the site"
                .into(),
        );
    }
    Ok(t.to_string())
}

/// The run file's text, unzipped.
fn plain_text(raw: &[u8]) -> Result<String, String> {
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut flate2::read::MultiGzDecoder::new(raw), &mut s)
            .map_err(|e| format!("the run file: {e}"))?;
        Ok(s)
    } else {
        String::from_utf8(raw.to_vec()).map_err(|_| "the run file is not UTF-8".to_string())
    }
}

/// That the text is a run file that is ready to upload: a header, and
/// finished unless `unfinished`. Returns the header.
fn check_run(text: &str, name: &str, unfinished: bool) -> Result<Value, String> {
    let first = text.lines().next().unwrap_or("");
    let header: Value = serde_json::from_str(first).map_err(|_| {
        format!(
            "{name} is not a run file: its first line is not the header `math-run run` writes \
             (a JSON object)"
        )
    })?;
    if header["format"].as_str() != Some("math-upload/1") {
        return Err(format!(
            "{name} is not a run file of format math-upload/1 (its header says {})",
            header["format"]
        ));
    }
    if header.get("finished").is_none() && !unfinished {
        return Err(format!(
            "the run {name} did not finish: `math-run resume {name}` goes on where it stopped. \
             To upload what there is (the problems not reached count as missing), add --unfinished"
        ));
    }
    Ok(header)
}

/// Where the record of an upload is kept: beside the run file.
fn uploaded_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(".uploaded.json");
    PathBuf::from(s)
}

/// In words: what the site's answer to the upload means.
fn explain_upload(status: u16, body: &Value, site: &str) -> String {
    let site = site.trim_end_matches('/');
    let detail = |v: &Value| match v {
        Value::String(s) => crate::session::cut(s, 200),
        Value::Object(o) => o
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| crate::session::cut(&v.to_string(), 200)),
        other => other.to_string(),
    };
    match status {
        401 => format!(
            "the site did not accept the token (401): it is wrong, or it was replaced by a newer \
             one. {site}/account shows the current token (log in); {site}/register is for a \
             first token"
        ),
        413 => "the file is over the 100 MB the site takes (413)".into(),
        415 => format!(
            "the site does not take this type of file (415); this is a bug of math-run: {}",
            detail(body)
        ),
        422 => {
            let errors: Vec<String> = body["errors"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|e| format!("  {}", e.as_str().unwrap_or("?")))
                        .collect()
                })
                .unwrap_or_else(|| vec![format!("  {}", detail(body))]);
            format!(
                "the site refused the file (422), and kept nothing; the first {} fault(s):\n{}",
                errors.len(),
                errors.join("\n")
            )
        }
        500..=599 => format!(
            "the site had an error ({status}): {}. The run may or may not have been stored: look \
             at your runs on {site} before uploading again",
            detail(body)
        ),
        _ => format!("the site answered {status}: {}", detail(body)),
    }
}

/// Upload a run file, gzipped on the way unless it is: `POST /api/runs`.
/// Prints the site's answer, and keeps a record beside the file so that the
/// same file is not uploaded twice by mistake.
pub fn upload_run(
    site: &str,
    token: Option<&str>,
    path: &Path,
    note: Option<&str>,
    unfinished: bool,
    again: bool,
) -> Result<(), String> {
    upload_with(
        site,
        token,
        path,
        note,
        unfinished,
        again,
        Duration::from_secs(UPLOAD_TIMEOUT),
    )
}

fn upload_with(
    site: &str,
    token: Option<&str>,
    path: &Path,
    note: Option<&str>,
    unfinished: bool,
    again: bool,
    timeout: Duration,
) -> Result<(), String> {
    let token = clean_token(token, site)?;
    let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.display().to_string();
    check_run(&plain_text(&raw)?, &name, unfinished)?;
    let record = uploaded_path(path);
    if !again && let Ok(text) = std::fs::read_to_string(&record) {
        let was: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        return Err(format!(
            "{name} was uploaded already, as run {} to {} on {}. Uploading it again makes a second \
             run on the site; --again does that",
            was["run"].as_str().unwrap_or("?"),
            was["site"].as_str().unwrap_or("?"),
            was["uploaded"].as_str().unwrap_or("?"),
        ));
    }
    let raw = match note {
        Some(note) => with_note(&raw, note)?,
        None => raw,
    };
    let body = if raw.starts_with(&[0x1f, 0x8b]) {
        raw
    } else {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gz.write_all(&raw).map_err(|e| e.to_string())?;
        gz.finish().map_err(|e| e.to_string())?
    };
    if body.len() > 100_000_000 {
        return Err(format!(
            "the upload is {} bytes, over the 100 MB the site takes",
            body.len()
        ));
    }
    let url = format!("{}/api/runs", site.trim_end_matches('/'));
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent(concat!("math-run/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_global(Some(timeout))
        .build();
    let agent = ureq::Agent::with_parts(config, DefaultConnector::default(), Resolver::default());
    let mut r = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/gzip")
        .send(&body[..])
        .map_err(|e| {
            if before_the_site(&e) {
                format!(
                    "{}. Nothing was uploaded: check the network and run the same command again",
                    explain(&url, &e)
                )
            } else {
                format!(
                    "{}. The site may have received the file: look at your runs on {} before \
                     uploading again",
                    explain(&url, &e),
                    site.trim_end_matches('/')
                )
            }
        })?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    if status != 202 {
        return Err(explain_upload(status, &v, site));
    }
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
    let run = v["run"].as_str().unwrap_or("?");
    eprintln!(
        "uploaded as run {run}; `math-run status {run}` tells how far the grading is{}",
        v["url"]
            .as_str()
            .map(|u| format!(", and {u} is its page"))
            .unwrap_or_default()
    );
    let record_text = json!({"run": run, "site": site, "uploaded": crate::runner::now_iso()});
    if let Err(e) = std::fs::write(&record, record_text.to_string()) {
        eprintln!(
            "warning: cannot write {}: {e}; the file can be uploaded again by mistake",
            record.display()
        );
    }
    Ok(())
}

/// The run file with `note` as its header's note (none where it is empty),
/// plain JSON lines.
pub fn with_note(raw: &[u8], note: &str) -> Result<Vec<u8>, String> {
    let text = plain_text(raw)?;
    let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let mut header: Value =
        serde_json::from_str(first).map_err(|e| format!("the run file's header: {e}"))?;
    let obj = header
        .as_object_mut()
        .ok_or("the run file's header is not a JSON object")?;
    match note.trim() {
        "" => obj.remove("note"),
        n => obj.insert("note".into(), Value::String(n.to_string())),
    };
    Ok(format!("{header}\n{rest}").into_bytes())
}

/// A run's status: `GET /api/runs/<run>`; prints the site's answer.
pub fn status_of(site: &str, token: Option<&str>, run: &str) -> Result<(), String> {
    let token = clean_token(token, site)?;
    let url = format!("{}/api/runs/{run}", site.trim_end_matches('/'));
    let mut r = agent(API_TIMEOUT)
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| explain(&url, &e))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    match status {
        200 => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            Ok(())
        }
        404 => Err(format!(
            "no run {run} among the runs of this token's owner (404)"
        )),
        _ => Err(explain_upload(status, &v, site)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn sets_and_removes_the_note() {
        let plain = b"{\"format\":\"math-upload/1\",\"note\":\"old\"}\n{\"id\":\"a\"}\n";
        let out = String::from_utf8(with_note(plain, " Six at a time. ").unwrap()).unwrap();
        assert_eq!(
            out,
            "{\"format\":\"math-upload/1\",\"note\":\"Six at a time.\"}\n{\"id\":\"a\"}\n"
        );
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(plain).unwrap();
        let out = String::from_utf8(with_note(&gz.finish().unwrap(), "").unwrap()).unwrap();
        assert_eq!(out, "{\"format\":\"math-upload/1\"}\n{\"id\":\"a\"}\n");
    }

    #[test]
    fn a_token_is_needed_and_checked() {
        let e = clean_token(None, "https://math.osp.no/").unwrap_err();
        assert!(e.contains("https://math.osp.no/register"), "{e}");
        assert!(e.contains("MATH_OSP_TOKEN"), "{e}");
        assert!(clean_token(Some("  \n"), "x").is_err());
        assert!(clean_token(Some("ab cd"), "x").is_err());
        assert_eq!(clean_token(Some(" abc123\n"), "x").unwrap(), "abc123");
    }

    #[test]
    fn only_a_finished_run_file_is_ready() {
        let done = "{\"format\":\"math-upload/1\",\"finished\":\"2026-11-07T00:00:00Z\"}\n";
        let cut = "{\"format\":\"math-upload/1\"}\n{\"id\":\"a\"}\n";
        assert!(check_run(done, "r.jsonl", false).is_ok());
        let e = check_run(cut, "r.jsonl", false).unwrap_err();
        assert!(e.contains("math-run resume r.jsonl"), "{e}");
        assert!(check_run(cut, "r.jsonl", true).is_ok());
        assert!(check_run("not json\n", "r.jsonl", true).is_err());
        assert!(check_run("{\"format\":\"other\"}\n", "r.jsonl", true).is_err());
        assert!(check_run("", "r.jsonl", true).is_err());
    }

    #[test]
    fn the_sites_answers_are_explained() {
        let s = "https://math.osp.no";
        let e = explain_upload(401, &json!({"error": "x"}), s);
        assert!(e.contains("https://math.osp.no/account"), "{e}");
        let e = explain_upload(422, &json!({"errors": ["line 1: bad", "line 9: worse"]}), s);
        assert!(e.contains("  line 1: bad\n  line 9: worse"), "{e}");
        let e = explain_upload(502, &Value::String("<html>".into()), s);
        assert!(e.contains("before uploading again"), "{e}");
    }

    /// A site that answers each request with `(status, body)` in turn.
    fn site(answers: Vec<(u16, &'static str)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = vec![0u8; 65536];
                let mut got = Vec::new();
                // the head, and then enough of the body
                loop {
                    let n = s.read(&mut buf).unwrap_or(0);
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got).to_string();
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len = text[..i]
                            .to_lowercase()
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if got.len() >= i + 4 + len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                seen.push(
                    String::from_utf8_lossy(&got)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string()
                        + " | "
                        + &String::from_utf8_lossy(&got)
                            .lines()
                            .filter(|l| l.to_lowercase().starts_with("authorization"))
                            .collect::<String>(),
                );
                write!(
                    s,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            seen
        });
        (url, handle)
    }

    fn run_file(dir: &Path, finished: bool) -> PathBuf {
        let p = dir.join("r.jsonl");
        let header = if finished {
            "{\"format\":\"math-upload/1\",\"finished\":\"2026-11-07T00:00:00Z\"}"
        } else {
            "{\"format\":\"math-upload/1\"}"
        };
        std::fs::write(
            &p,
            format!("{header}\n{{\"id\":\"a\",\"status\":\"ok\"}}\n"),
        )
        .unwrap();
        p
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("math-run-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn upload_wrong_token_then_good_then_not_twice() {
        let dir = tmp("upload");
        let file = run_file(&dir, true);
        let (url, server) = site(vec![
            (401, "{\"error\":\"no\"}"),
            (
                202,
                "{\"run\":\"r-abc\",\"status\":\"processing\",\"url\":\"u\"}",
            ),
            (
                202,
                "{\"run\":\"r-def\",\"status\":\"processing\",\"url\":\"u\"}",
            ),
        ]);
        let t = Duration::from_secs(20);
        let e = upload_with(&url, Some("wrong"), &file, None, false, false, t).unwrap_err();
        assert!(e.contains("did not accept the token"), "{e}");
        assert!(!uploaded_path(&file).exists());
        upload_with(&url, Some("good"), &file, None, false, false, t).unwrap();
        assert!(uploaded_path(&file).exists());
        // the same file again is refused, without a request
        let e = upload_with(&url, Some("good"), &file, None, false, false, t).unwrap_err();
        assert!(e.contains("r-abc") && e.contains("--again"), "{e}");
        // and goes with --again
        upload_with(&url, Some("good"), &file, None, false, true, t).unwrap();
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(seen[0].starts_with("POST /api/runs"), "{seen:?}");
        assert!(seen[0].to_lowercase().contains("bearer wrong"), "{seen:?}");
        assert!(seen[1].to_lowercase().contains("bearer good"), "{seen:?}");
    }

    #[test]
    fn an_unfinished_run_is_not_uploaded() {
        let dir = tmp("unfinished");
        let file = run_file(&dir, false);
        // no site is asked: the port is nobody's
        let e = upload_with(
            "http://127.0.0.1:1",
            Some("t"),
            &file,
            None,
            false,
            false,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(e.contains("did not finish"), "{e}");
    }

    #[test]
    fn no_network_says_nothing_was_uploaded() {
        let dir = tmp("refused");
        let file = run_file(&dir, true);
        let e = upload_with(
            "http://127.0.0.1:1",
            Some("t"),
            &file,
            None,
            false,
            false,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(e.contains("Nothing was uploaded"), "{e}");
        assert!(!uploaded_path(&file).exists());
    }

    #[test]
    fn a_site_that_does_not_answer_times_out() {
        let dir = tmp("timeout");
        let file = run_file(&dir, true);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        // accepts, and says nothing
        let held = std::thread::spawn(move || {
            let c = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(4));
            drop(c);
        });
        let t0 = std::time::Instant::now();
        let e = upload_with(
            &url,
            Some("t"),
            &file,
            None,
            false,
            false,
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
        assert!(e.contains("did not answer in time"), "{e}");
        assert!(e.contains("before uploading again"), "{e}");
        assert!(!uploaded_path(&file).exists());
        held.join().unwrap();
    }
}
