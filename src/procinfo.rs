//! The processes of a system, and their CPU time and memory, from the
//! operating system. On Unix they are a process group (the system is started
//! as a group of its own), read on Linux from `/proc`, on macOS from libproc;
//! on Windows a job object, which the system is put in before it runs, so
//! that the processes it starts (as Maple's `cmaple` starts the kernel
//! `mserver`) are in it too. Elsewhere the times are unknown.

#[cfg(unix)]
use std::process::{Child, Command};

/// The processes of one session of a system.
#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub pgid: i32,
}

#[cfg(unix)]
impl Group {
    /// Set the command up to start the system as a process group of its own.
    pub fn prepare(cmd: &mut Command) {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    /// The group of a system started by a command set up by `prepare`.
    pub fn of(child: &Child) -> std::io::Result<Group> {
        Ok(Group {
            pgid: child.id() as i32,
        })
    }

    /// Kill every process of the group.
    pub fn kill(&self) {
        unsafe {
            libc::killpg(self.pgid, libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
pub use win::Group;

/// The user and system CPU seconds of the group's processes: on Unix those of
/// its live processes, with those of their children that they have waited
/// for; on Windows of every process that was in the job, the ended ones too.
pub fn group_cpu_seconds(g: &Group) -> Option<f64> {
    #[cfg(unix)]
    return imp::group_cpu_seconds(g.pgid);
    #[cfg(windows)]
    return win::cpu_seconds(g);
}

/// The resident memory (on Windows the working set), in bytes, of the
/// group's processes.
pub fn group_memory_bytes(g: &Group) -> Option<u64> {
    #[cfg(unix)]
    return imp::group_memory_bytes(g.pgid);
    #[cfg(windows)]
    return win::memory_bytes(g);
}

#[cfg(target_os = "linux")]
mod imp {
    pub fn group_memory_bytes(pgid: i32) -> Option<u64> {
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64;
        let mut total = 0u64;
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
            let Some(close) = stat.rfind(')') else {
                continue;
            };
            let f: Vec<&str> = stat[close + 1..].split_whitespace().collect();
            if f.len() < 22 || f[2].parse::<i32>().ok() != Some(pgid) {
                continue;
            }
            found = true;
            // rss, in pages, is field 24 of stat: f[21]
            total += f[21].parse::<u64>().unwrap_or(0) * page;
        }
        found.then_some(total)
    }

    pub fn group_cpu_seconds(pgid: i32) -> Option<f64> {
        let tick = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
        if tick <= 0.0 {
            return None;
        }
        let mut ticks: u64 = 0;
        let mut ns: u64 = 0;
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
            let field = |i: usize| f[i].parse::<u64>().unwrap_or(0);
            // the CPU clock of the process, in nanoseconds, where the kernel
            // gives it; else the clock ticks of stat
            match on_cpu_ns(&name) {
                Some(n) => ns += n,
                None => ticks += field(11) + field(12),
            }
            // the children it has waited for
            ticks += field(13) + field(14);
        }
        found.then(|| ticks as f64 / tick + ns as f64 / 1e9)
    }

    /// The process's CPU clock: every thread's time, the ended ones too, to
    /// the nanosecond.
    fn on_cpu_ns(pid: &str) -> Option<u64> {
        let pid: libc::pid_t = pid.parse().ok()?;
        let mut clock: libc::clockid_t = 0;
        if unsafe { libc::clock_getcpuclockid(pid, &mut clock) } != 0 {
            return None;
        }
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(clock, &mut ts) } != 0 {
            return None;
        }
        Some(ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64)
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

    pub fn group_memory_bytes(pgid: i32) -> Option<u64> {
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
        let mut total = 0u64;
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
                total += info.ri_resident_size;
            }
        }
        found.then_some(total)
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

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
mod imp {
    pub fn group_memory_bytes(_pgid: i32) -> Option<u64> {
        None
    }

    pub fn group_cpu_seconds(_pgid: i32) -> Option<f64> {
        None
    }
}

/// Windows: the system's processes in a job object. The system is started
/// suspended, put in the job, and only then let run, so that every process it
/// starts is in the job as well. The job is killed when its last handle
/// closes (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`): a runner that ends in any
/// way, Ctrl-C or killed, leaves no system computing on.
#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        TerminateJobObject,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, OpenProcess, OpenThread,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ, ResumeThread, THREAD_SUSPEND_RESUME,
    };

    /// The job object of one session of a system.
    pub struct Group {
        job: HANDLE,
    }

    // The handle is the job's, owned by the group alone; the job functions
    // may be called from any thread.
    unsafe impl Send for Group {}
    unsafe impl Sync for Group {}

    impl Drop for Group {
        fn drop(&mut self) {
            // the last handle: the job's processes are killed with it
            unsafe { CloseHandle(self.job) };
        }
    }

    impl Group {
        /// Set the command up to start the system suspended (it runs once
        /// it is in the job), in a process group of its own, which the
        /// console's Ctrl-C does not reach.
        pub fn prepare(cmd: &mut Command) {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
        }

        /// Put a system started by a command set up by `prepare` in a new
        /// job, and let it run.
        pub fn of(child: &Child) -> io::Result<Group> {
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let g = Group { job };
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            if unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) } == 0 {
                return Err(io::Error::last_os_error());
            }
            resume(child.id())?;
            Ok(g)
        }

        /// Kill every process of the job.
        pub fn kill(&self) {
            unsafe { TerminateJobObject(self.job, 1) };
        }
    }

    /// Let the suspended threads of a process run (it has one, the main
    /// thread, as it was started suspended).
    fn resume(pid: u32) -> io::Result<()> {
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut te: THREADENTRY32 = unsafe { zeroed() };
        te.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut resumed = 0;
        let mut more = unsafe { Thread32First(snap, &mut te) } != 0;
        while more {
            if te.th32OwnerProcessID == pid {
                let t = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, te.th32ThreadID) };
                if !t.is_null() {
                    if unsafe { ResumeThread(t) } != u32::MAX {
                        resumed += 1;
                    }
                    unsafe { CloseHandle(t) };
                }
            }
            te.dwSize = size_of::<THREADENTRY32>() as u32;
            more = unsafe { Thread32Next(snap, &mut te) } != 0;
        }
        unsafe { CloseHandle(snap) };
        if resumed == 0 {
            return Err(io::Error::other(
                "the system was started, but its process could not be let run",
            ));
        }
        Ok(())
    }

    pub fn cpu_seconds(g: &Group) -> Option<f64> {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                g.job,
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut c_void,
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        // in units of 100 ns
        (ok != 0).then(|| (info.TotalUserTime + info.TotalKernelTime) as f64 / 1e7)
    }

    /// JOBOBJECT_BASIC_PROCESS_ID_LIST, with room for the ids.
    #[repr(C)]
    struct IdList {
        assigned: u32,
        listed: u32,
        ids: [usize; 1024],
    }

    pub fn memory_bytes(g: &Group) -> Option<u64> {
        let mut list: Box<IdList> = Box::new(unsafe { zeroed() });
        let ok = unsafe {
            QueryInformationJobObject(
                g.job,
                JobObjectBasicProcessIdList,
                &mut *list as *mut IdList as *mut c_void,
                size_of::<IdList>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }
        let n = (list.listed as usize).min(list.ids.len());
        let mut total = 0u64;
        let mut found = false;
        for &pid in &list.ids[..n] {
            let mut h = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                    0,
                    pid as u32,
                )
            };
            if h.is_null() {
                h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32) };
            }
            if h.is_null() {
                continue;
            }
            let mut c: PROCESS_MEMORY_COUNTERS = unsafe { zeroed() };
            c.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if unsafe { K32GetProcessMemoryInfo(h, &mut c, c.cb) } != 0 {
                found = true;
                total += c.WorkingSetSize as u64;
            }
            unsafe { CloseHandle(h) };
        }
        found.then_some(total)
    }
}
