//! The suite: read from a file, plain or gzipped, or fetched from the site
//! (`GET /api/suites`) into a cache, its hash checked.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::problem::Problem;

pub struct Suite {
    pub version: String,
    /// The hash of the whole suite, unzipped: the header's `suite_sha256`.
    pub sha256: String,
    pub problems: Vec<Problem>,
}

/// Open a file, gzipped or not, as a reader of its plain bytes.
pub fn open(path: &Path) -> Result<Box<dyn Read>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut magic = [0u8; 2];
    let n = f.read(&mut magic).map_err(|e| e.to_string())?;
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if n == 2 && magic == [0x1f, 0x8b] {
        Ok(Box::new(flate2::read::MultiGzDecoder::new(f)))
    } else {
        Ok(Box::new(f))
    }
}

/// The SHA-256 of the file's plain bytes, in lower case hex.
pub fn sha256_of(path: &Path) -> Result<String, String> {
    let mut r = open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = r.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Read the problems of a suite file.
pub fn read_problems(path: &Path) -> Result<Vec<Problem>, String> {
    let r = BufReader::new(open(path)?);
    let mut out = Vec::new();
    for (i, line) in r.lines().enumerate() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line)
            .map_err(|e| format!("{} line {}: {e}", path.display(), i + 1))?;
        out.push(
            Problem::from_json(&v)
                .map_err(|e| format!("{} line {}: {e}", path.display(), i + 1))?,
        );
    }
    Ok(out)
}

/// A suite from a local file. Its version is given, or read from the
/// `manifest.json` beside it.
pub fn local(path: &Path, version: Option<&str>) -> Result<Suite, String> {
    let version = match version {
        Some(v) => v.to_string(),
        None => {
            let manifest = path.with_file_name("manifest.json");
            let m: Value = serde_json::from_str(
                &std::fs::read_to_string(&manifest)
                    .map_err(|_| "give the suite's version with --suite-version, or a manifest.json beside the file".to_string())?,
            )
            .map_err(|e| format!("{}: {e}", manifest.display()))?;
            m["version"]
                .as_str()
                .ok_or("the manifest has no version")?
                .to_string()
        }
    };
    Ok(Suite {
        version,
        sha256: sha256_of(path)?,
        problems: read_problems(path)?,
    })
}

/// The problems of the subset, the calibration subset first.
pub fn ordered(problems: &[Problem], subset: &str) -> Vec<Problem> {
    let mut calibration: Vec<Problem> =
        problems.iter().filter(|p| p.calibration).cloned().collect();
    let rest = problems
        .iter()
        .filter(|p| !p.calibration && (subset == "full" || p.quick))
        .cloned();
    calibration.extend(rest);
    calibration
}

/// The cache of fetched suites: `$MATH_RUN_CACHE`, or the user's cache.
pub fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("MATH_RUN_CACHE") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    if cfg!(target_os = "macos") {
        PathBuf::from(home).join("Library/Caches/math-run")
    } else {
        std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(home).join(".cache"))
            .join("math-run")
    }
}

/// Fetch a suite from the site: the newest, or `version`. The whole suite
/// is fetched where the subset is `full`, the quick file where it is
/// `quick`; either is checked against its hash, and kept in the cache.
pub fn fetch(site: &str, version: Option<&str>, subset: &str) -> Result<Suite, String> {
    let list = crate::net::get_json(&format!("{}/api/suites", site.trim_end_matches('/')))?;
    let suites = list["suites"]
        .as_array()
        .ok_or("the site lists no suites")?;
    let s = match version {
        Some(v) => suites
            .iter()
            .find(|s| s["version"].as_str() == Some(v))
            .ok_or_else(|| format!("the site has no suite {v}"))?,
        None => suites.first().ok_or("the site has no suite")?,
    };
    let version = s["version"]
        .as_str()
        .ok_or("a suite without a version")?
        .to_string();
    let full_sha = s["sha256"]
        .as_str()
        .ok_or("a suite without a hash")?
        .to_string();
    let (url, sha, name) = if subset == "quick" {
        (
            s["files"]["quick"]
                .as_str()
                .ok_or("no quick file")?
                .to_string(),
            s["quick"]["sha256"]
                .as_str()
                .ok_or("no quick hash")?
                .to_string(),
            "quick.jsonl.gz",
        )
    } else {
        (
            s["files"]["suite"]
                .as_str()
                .ok_or("no suite file")?
                .to_string(),
            full_sha.clone(),
            "suite.jsonl.gz",
        )
    };
    let dir = cache_dir().join("suites").join(&version);
    let path = dir.join(name);
    let cached_ok = path.exists() && sha256_of(&path).ok().as_deref() == Some(sha.as_str());
    if !cached_ok {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let tmp = dir.join(format!("{name}.part"));
        eprintln!("fetching {url}");
        crate::net::download(&url, &tmp)?;
        let got = sha256_of(&tmp)?;
        if got != sha {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!(
                "the suite's hash is {got}, not {sha} as the site says"
            ));
        }
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    }
    if let Some(m) = s["files"]["manifest"].as_str() {
        if let Ok(v) = crate::net::get_json(m) {
            let _ = std::fs::write(dir.join("manifest.json"), v.to_string());
        }
    }
    Ok(Suite {
        version,
        sha256: full_sha,
        problems: read_problems(&path)?,
    })
}
