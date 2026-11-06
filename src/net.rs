//! The site's API (section 6 of the formats): the suites, and the upload.

use std::io::Write;
use std::path::Path;

use serde_json::Value;
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

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent(concat!("math-run/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::default(), Resolver::default())
}

pub fn get_json(url: &str) -> Result<Value, String> {
    let mut r = agent().get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let status = r.status().as_u16();
    let body = r
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("{url}: {e}"))?;
    if status != 200 {
        return Err(format!("{url}: {status} {body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("{url}: {e}"))
}

pub fn download(url: &str, to: &Path) -> Result<(), String> {
    let mut r = agent().get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let status = r.status().as_u16();
    if status != 200 {
        return Err(format!("{url}: {status}"));
    }
    let mut f = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut body = r.body_mut().with_config().limit(u64::MAX).reader();
    std::io::copy(&mut body, &mut f).map_err(|e| format!("{url}: {e}"))?;
    f.flush().map_err(|e| e.to_string())
}

/// Upload a run file, gzipped on the way unless it is: `POST /api/runs`.
/// Returns the status and the site's answer.
pub fn upload(
    site: &str,
    token: &str,
    path: &Path,
    note: Option<&str>,
) -> Result<(u16, Value), String> {
    let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
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
    let mut r = agent()
        .post(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/gzip")
        .send(&body[..])
        .map_err(|e| format!("{url}: {e}"))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().unwrap_or_default();
    let v = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Ok((status, v))
}

/// The run file with `note` as its header's note (none where it is empty),
/// plain JSON lines.
pub fn with_note(raw: &[u8], note: &str) -> Result<Vec<u8>, String> {
    let text = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut flate2::read::MultiGzDecoder::new(raw), &mut s)
            .map_err(|e| format!("the run file: {e}"))?;
        s
    } else {
        String::from_utf8(raw.to_vec()).map_err(|_| "the run file is not UTF-8".to_string())?
    };
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

/// A run's status: `GET /api/runs/<run>`.
pub fn status(site: &str, token: &str, run: &str) -> Result<(u16, Value), String> {
    let url = format!("{}/api/runs/{run}", site.trim_end_matches('/'));
    let mut r = agent()
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().unwrap_or_default();
    Ok((
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    ))
}

#[cfg(test)]
mod note_tests {
    use super::with_note;
    use std::io::Write;

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
}
