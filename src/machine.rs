//! The machine of the header (section 5.1): the CPU's model, its cores, the
//! memory and the operating system, read from the system.

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct Machine {
    pub cpu: String,
    pub cores: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_gb: Option<u64>,
    /// The hardware threads, where they are more than the cores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<usize>,
    /// The machine's id, the same on every run of this machine: see `id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// The machine's id: the first 16 hex digits of the SHA-256 of
/// `math.osp.no machine <raw>`, where raw is the operating system's own id
/// of the machine, macOS's IOPlatformUUID, Linux's /etc/machine-id or
/// Windows's MachineGuid. Only the hash leaves the machine. None where there
/// is no such id, or where `MATH_RUN_NO_MACHINE_ID` is set (to anything but
/// empty or `0`).
pub fn id() -> Option<String> {
    let opt_out = std::env::var("MATH_RUN_NO_MACHINE_ID").ok();
    if opted_out(opt_out.as_deref()) {
        return None;
    }
    id_of(&raw_id()?)
}

/// `MATH_RUN_NO_MACHINE_ID` set to anything but empty or `0`.
fn opted_out(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// The id of a raw machine id: none where it is empty.
fn id_of(raw: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let hash = Sha256::digest(format!("math.osp.no machine {raw}").as_bytes());
    Some(hash.iter().take(8).map(|b| format!("{b:02x}")).collect())
}

#[cfg(windows)]
fn raw_id() -> Option<String> {
    let raw = win::reg_string(r"SOFTWARE\Microsoft\Cryptography", "MachineGuid")?;
    let raw = raw.trim().to_string();
    (!raw.is_empty()).then_some(raw)
}

#[cfg(unix)]
fn raw_id() -> Option<String> {
    let raw = if cfg!(target_os = "macos") {
        let out = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.contains("\"IOPlatformUUID\""))
            .and_then(|l| l.rsplit('"').nth(1).map(str::to_string))?
    } else {
        // systemd's, else dbus's (Debian without systemd, older containers)
        std::fs::read_to_string("/etc/machine-id")
            .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
            .ok()?
    };
    let raw = raw.trim().to_string();
    (!raw.is_empty()).then_some(raw)
}

/// `Windows 10.0.26200 x86_64 (Windows 11 Enterprise 25H2)`, from the
/// registry's CurrentVersion.
#[cfg(windows)]
pub fn platform() -> String {
    win::platform().chars().take(200).collect()
}

/// `Linux 6.8.0 x86_64`, from uname.
#[cfg(unix)]
pub fn platform() -> String {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } != 0 {
        return std::env::consts::OS.to_string();
    }
    let field = |f: &[libc::c_char]| {
        let bytes: Vec<u8> = f
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).to_string()
    };
    let mut s = format!(
        "{} {} {}",
        field(&u.sysname),
        field(&u.release),
        field(&u.machine)
    );
    if let Some(name) = os_name() {
        s = format!("{s} ({name})");
    }
    s.chars().take(200).collect()
}

/// The distribution's or the release's name: `Debian GNU/Linux 13
/// (trixie)`, `macOS 15.6`.
#[cfg(unix)]
fn os_name() -> Option<String> {
    if cfg!(target_os = "macos") {
        let v = sysctl_string("kern.osproductversion")?;
        return Some(format!("macOS {v}"));
    }
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

pub fn detect() -> Machine {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let (cpu, cores, memory) = if cfg!(windows) {
        windows()
    } else if cfg!(target_os = "macos") {
        (
            sysctl_string("machdep.cpu.brand_string"),
            sysctl_u64("hw.physicalcpu").map(|n| n as usize),
            sysctl_u64("hw.memsize"),
        )
    } else {
        linux()
    };
    let cores = cores.unwrap_or(threads).max(1);
    Machine {
        cpu: cpu.unwrap_or_else(|| format!("unknown {} CPU", std::env::consts::ARCH)),
        cores,
        memory_gb: memory.map(|b| (b as f64 / (1u64 << 30) as f64).round() as u64),
        threads: (threads > cores).then_some(threads),
        id: id(),
    }
}

fn linux() -> (Option<String>, Option<usize>, Option<u64>) {
    let info = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let mut model = None;
    let mut cores = std::collections::HashSet::new();
    let (mut phys, mut core) = (String::new(), String::new());
    for line in info.lines() {
        let (k, v) = match line.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => {
                if !core.is_empty() {
                    cores.insert((phys.clone(), core.clone()));
                }
                phys.clear();
                core.clear();
                continue;
            }
        };
        match k {
            "model name" | "Model" | "Hardware" if model.is_none() && !v.is_empty() => {
                model = Some(v.to_string())
            }
            "physical id" => phys = v.to_string(),
            "core id" => core = v.to_string(),
            _ => {}
        }
    }
    if !core.is_empty() {
        cores.insert((phys, core));
    }
    if model.is_none() {
        model = lscpu_model();
    }
    let memory = std::fs::read_to_string("/proc/meminfo").ok().and_then(|m| {
        m.lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            .map(|kb| kb * 1024)
    });
    let cores = (!cores.is_empty()).then_some(cores.len());
    (model, cores, memory)
}

/// The model on ARM machines, whose /proc/cpuinfo names none.
fn lscpu_model() -> Option<String> {
    let out = std::process::Command::new("lscpu").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let vendor = text
        .lines()
        .find_map(|l| l.strip_prefix("Vendor ID:"))
        .map(|v| v.trim().to_string());
    let model = text
        .lines()
        .find_map(|l| l.strip_prefix("Model name:"))
        .map(|v| v.trim().to_string())?;
    Some(match vendor {
        Some(v) if !model.contains(&v) => format!("{v} {model}"),
        _ => model,
    })
}

#[cfg(windows)]
fn windows() -> (Option<String>, Option<usize>, Option<u64>) {
    (win::cpu(), win::physical_cores(), win::memory_bytes())
}

#[cfg(not(windows))]
fn windows() -> (Option<String>, Option<usize>, Option<u64>) {
    (None, None, None)
}

/// Windows: the CPU, the version and the machine's id from the registry,
/// the cores and the memory from the system information API.
#[cfg(windows)]
mod win {
    use std::mem::{size_of, zeroed};

    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
    };
    use windows_sys::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, GlobalMemoryStatusEx, MEMORYSTATUSEX,
        RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A string value of HKEY_LOCAL_MACHINE (its 64-bit view).
    pub fn reg_string(key: &str, name: &str) -> Option<String> {
        let (k, n) = (wide(key), wide(name));
        let mut buf = vec![0u16; 512];
        let mut bytes = (buf.len() * 2) as u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                k.as_ptr(),
                n.as_ptr(),
                RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if r != 0 {
            return None;
        }
        let len = (bytes as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        let s = s.trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// A DWORD value of HKEY_LOCAL_MACHINE (its 64-bit view).
    pub fn reg_dword(key: &str, name: &str) -> Option<u32> {
        let (k, n) = (wide(key), wide(name));
        let mut v: u32 = 0;
        let mut bytes = 4u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                k.as_ptr(),
                n.as_ptr(),
                RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY,
                std::ptr::null_mut(),
                (&mut v as *mut u32).cast(),
                &mut bytes,
            )
        };
        (r == 0).then_some(v)
    }

    const CURRENT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

    pub fn platform() -> String {
        let major = reg_dword(CURRENT, "CurrentMajorVersionNumber");
        let minor = reg_dword(CURRENT, "CurrentMinorVersionNumber");
        let build = reg_string(CURRENT, "CurrentBuildNumber");
        let version = match (major, minor, &build) {
            (Some(a), Some(b), Some(c)) => format!("{a}.{b}.{c}"),
            (_, _, Some(c)) => c.clone(),
            _ => "unknown".into(),
        };
        let mut s = format!("Windows {version} {}", std::env::consts::ARCH);
        // ProductName says Windows 10 on Windows 11 too: the build tells
        if let Some(mut name) = reg_string(CURRENT, "ProductName") {
            let build: u32 = build.as_deref().and_then(|b| b.parse().ok()).unwrap_or(0);
            if build >= 22000 && name.starts_with("Windows 10") {
                name = name.replacen("Windows 10", "Windows 11", 1);
            }
            if let Some(d) = reg_string(CURRENT, "DisplayVersion") {
                name = format!("{name} {d}");
            }
            s = format!("{s} ({name})");
        }
        s
    }

    pub fn cpu() -> Option<String> {
        reg_string(
            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "ProcessorNameString",
        )
    }

    /// The physical cores: the processor cores of
    /// GetLogicalProcessorInformationEx.
    pub fn physical_cores() -> Option<usize> {
        let mut len = 0u32;
        unsafe {
            GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut len)
        };
        if len == 0 {
            return None;
        }
        // 8-aligned room for the records
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let ok = unsafe {
            GetLogicalProcessorInformationEx(
                RelationProcessorCore,
                buf.as_mut_ptr()
                    .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>(),
                &mut len,
            )
        };
        if ok == 0 {
            return None;
        }
        let base = buf.as_ptr().cast::<u8>();
        let (mut off, mut cores) = (0usize, 0usize);
        while off + size_of::<u32>() * 2 <= len as usize {
            let rec = unsafe {
                &*(base
                    .add(off)
                    .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>())
            };
            if rec.Size == 0 {
                break;
            }
            if rec.Relationship == RelationProcessorCore {
                cores += 1;
            }
            off += rec.Size as usize;
        }
        (cores > 0).then_some(cores)
    }

    pub fn memory_bytes() -> Option<u64> {
        let mut m: MEMORYSTATUSEX = unsafe { zeroed() };
        m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then_some(m.ullTotalPhys)
    }
}

fn sysctl_string(name: &str) -> Option<String> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", name])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn sysctl_u64(name: &str) -> Option<u64> {
    sysctl_string(name)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_a_salted_hash_of_the_raw_id() {
        // sha256("math.osp.no machine some-raw-id"), first 16 hex digits
        assert_eq!(id_of(" some-raw-id\n").as_deref(), Some("63a6532068408cf4"));
        assert_eq!(id_of("some-raw-id").as_deref(), Some("63a6532068408cf4"));
        assert_eq!(id_of("  "), None);
    }

    #[test]
    fn opt_out_is_anything_but_empty_or_zero() {
        assert!(!opted_out(None));
        assert!(!opted_out(Some("")));
        assert!(!opted_out(Some("0")));
        assert!(opted_out(Some("1")));
        assert!(opted_out(Some("yes")));
    }
}
