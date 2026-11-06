//! The release of math-run: `cargo xtask release` builds the binaries for
//! Linux x86_64, Linux aarch64 and macOS arm64, and puts each in a tar.gz
//! in `dist/`, with `SHA256SUMS`.
//!
//! The Linux binaries are static (musl), so that they run on any
//! distribution; they are cross-compiled with cargo-zigbuild, which needs
//! zig:
//!
//!     cargo install cargo-zigbuild
//!     brew install zig            (or a zig release from ziglang.org on the PATH)
//!
//! The macOS binary is built natively, on an Apple silicon Mac.

use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use sha2::{Digest, Sha256};

/// The targets: the Rust target, and the name of the archive.
const TARGETS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-musl", "linux-x86_64"),
    ("aarch64-unknown-linux-musl", "linux-aarch64"),
    ("aarch64-apple-darwin", "macos-arm64"),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("release") => {
            let only: Vec<&str> = args[1..].iter().map(String::as_str).collect();
            if let Err(e) = release(&only) {
                eprintln!("xtask: {e}");
                exit(1);
            }
        }
        _ => {
            eprintln!("usage: cargo xtask release [linux-x86_64] [linux-aarch64] [macos-arm64]");
            exit(2);
        }
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn run(cmd: &mut Command) -> Result<(), String> {
    eprintln!("$ {cmd:?}");
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed: {status}"))
    }
}

fn version(root: &Path) -> Result<String, String> {
    let toml = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    toml.lines()
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim_matches('"').to_string())
        .ok_or_else(|| "no version in Cargo.toml".into())
}

fn release(only: &[&str]) -> Result<(), String> {
    let root = root();
    let version = version(&root)?;
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    let mut sums = String::new();
    for &(target, name) in TARGETS {
        if !only.is_empty() && !only.contains(&name) {
            continue;
        }
        let native = cfg!(all(target_os = "macos", target_arch = "aarch64"))
            && target == "aarch64-apple-darwin";
        if !native && target.ends_with("apple-darwin") {
            eprintln!("skipping {name}: build it on an Apple silicon Mac");
            continue;
        }
        let _ = Command::new("rustup")
            .args(["target", "add", target])
            .status();
        let mut cargo = Command::new("cargo");
        cargo.current_dir(&root);
        if native {
            cargo.args([
                "build",
                "--release",
                "--package",
                "math-run",
                "--target",
                target,
            ]);
        } else {
            cargo.args([
                "zigbuild",
                "--release",
                "--package",
                "math-run",
                "--target",
                target,
            ]);
        }
        run(&mut cargo).map_err(|e| {
            if native {
                e
            } else {
                format!("{e}\n(cross builds need cargo-zigbuild and zig, see xtask/src/main.rs)")
            }
        })?;
        let binary = root
            .join("target")
            .join(target)
            .join("release")
            .join("math-run");
        let stem = format!("math-run-{version}-{name}");
        let stage = dist.join(&stem);
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
        std::fs::copy(&binary, stage.join("math-run"))
            .map_err(|e| format!("{}: {e}", binary.display()))?;
        for file in ["README.md", "LICENSE"] {
            std::fs::copy(root.join(file), stage.join(file)).map_err(|e| e.to_string())?;
        }
        let archive = format!("{stem}.tar.gz");
        run(Command::new("tar")
            .current_dir(&dist)
            // no AppleDouble files in archives made on macOS
            .env("COPYFILE_DISABLE", "1")
            .args(["-czf", &archive, &stem]))?;
        std::fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(dist.join(&archive)).map_err(|e| e.to_string())?;
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        sums.push_str(&format!("{hash}  {archive}\n"));
        eprintln!("built dist/{archive}");
    }
    std::fs::write(dist.join("SHA256SUMS"), sums).map_err(|e| e.to_string())?;
    eprintln!("the archives and SHA256SUMS are in {}", dist.display());
    Ok(())
}
