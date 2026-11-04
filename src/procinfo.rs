//! The CPU time of a process group, from the operating system: on Linux
//! from `/proc`, on macOS from libproc. Elsewhere it is unknown.

/// The user and system CPU seconds of the processes of the group `pgid`,
/// with those of their children that they have waited for.
pub fn group_cpu_seconds(pgid: i32) -> Option<f64> {
    imp::group_cpu_seconds(pgid)
}

#[cfg(target_os = "linux")]
mod imp {
    pub fn group_cpu_seconds(pgid: i32) -> Option<f64> {
        let tick = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
        if tick <= 0.0 {
            return None;
        }
        let mut ticks: u64 = 0;
        let mut found = false;
        for entry in std::fs::read_dir("/proc").ok()?.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{name}/stat")) else {
                continue;
            };
            // the fields after the command, which is in parentheses
            let Some(close) = stat.rfind(')') else {
                continue;
            };
            let f: Vec<&str> = stat[close + 1..].split_whitespace().collect();
            // f[0] is field 3 (state); pgrp is field 5, utime..cstime 14..17
            if f.len() < 15 || f[2].parse::<i32>().ok() != Some(pgid) {
                continue;
            }
            found = true;
            for i in 11..15 {
                ticks += f[i].parse::<u64>().unwrap_or(0);
            }
        }
        found.then(|| ticks as f64 / tick)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::OnceLock;

    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }

    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> i32;
    }

    /// Nanoseconds per unit of the CPU times libproc gives.
    fn timebase() -> f64 {
        static TB: OnceLock<f64> = OnceLock::new();
        *TB.get_or_init(|| {
            let mut info = Timebase { numer: 0, denom: 0 };
            unsafe { mach_timebase_info(&mut info) };
            if info.denom == 0 {
                1.0
            } else {
                info.numer as f64 / info.denom as f64
            }
        })
    }

    pub fn group_cpu_seconds(pgid: i32) -> Option<f64> {
        let mut pids = vec![0 as libc::pid_t; 1024];
        let n = unsafe {
            libc::proc_listpgrppids(
                pgid,
                pids.as_mut_ptr() as *mut libc::c_void,
                (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
            )
        };
        if n <= 0 {
            return None;
        }
        let mut ns: f64 = 0.0;
        let mut found = false;
        for &pid in pids.iter().filter(|&&p| p > 0) {
            let mut info: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
            let r = unsafe {
                libc::proc_pid_rusage(
                    pid,
                    libc::RUSAGE_INFO_V0,
                    &mut info as *mut libc::rusage_info_v0 as *mut libc::rusage_info_t,
                )
            };
            if r == 0 {
                found = true;
                ns += (info.ri_user_time + info.ri_system_time) as f64 * timebase();
            }
        }
        found.then(|| ns / 1e9)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    pub fn group_cpu_seconds(_pgid: i32) -> Option<f64> {
        None
    }
}
