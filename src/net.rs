//! The site's API (section 6 of the formats): the suites, and the upload.

use std::io::Write;
use std::path::Path;

use serde_json::Value;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver as _};
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
pub fn upload(site: &str, token: &str, path: &Path) -> Result<(u16, Value), String> {
    let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
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
