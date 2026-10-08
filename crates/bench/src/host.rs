//! Host probes: peak memory and CPU time of the current process, and a description of the machine.
//!
//! Both read what the operating system already tracks, without `unsafe` or FFI in this crate:
//! `/proc` on Linux, and on Windows a PowerShell query of this process's own counters (the process
//! is still alive while it is queried, so the OS-maintained peak working set is exact). Other
//! platforms report `None` — absent, not estimated.

use std::process::Command;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ProcessProbe {
    /// Peak resident set size (Linux `VmHWM`) or peak working set (Windows), in bytes.
    pub peak_rss_bytes: Option<u64>,
    /// User + kernel CPU time of the whole process so far, summed over threads. Less sensitive than
    /// wall time to other load on the machine, so it is the better A/B signal on a busy host.
    pub cpu_ms: Option<f64>,
}

/// Probes this process. Call it at the end of a measured run, while the process is still alive.
pub fn probe_self() -> ProcessProbe {
    if cfg!(target_os = "linux") {
        let status = std::fs::read_to_string("/proc/self/status").ok();
        let peak_rss_bytes = status.as_deref().and_then(|s| {
            let line = s.lines().find(|l| l.starts_with("VmHWM:"))?;
            line.split_whitespace().nth(1)?.parse::<u64>().ok().map(|kib| kib * 1024)
        });
        // Fields 14 and 15 (utime, stime) in clock ticks; USER_HZ is 100 on every mainstream
        // Linux build, and reading it properly needs `sysconf` (FFI).
        let cpu_ms = std::fs::read_to_string("/proc/self/stat").ok().and_then(|stat| {
            let after_comm = stat.rsplit_once(')')?.1;
            let fields: Vec<&str> = after_comm.split_whitespace().collect();
            let utime: f64 = fields.get(11)?.parse().ok()?;
            let stime: f64 = fields.get(12)?.parse().ok()?;
            Some((utime + stime) * 10.0)
        });
        ProcessProbe { peak_rss_bytes, cpu_ms }
    } else if cfg!(windows) {
        let query = format!(
            "$p = Get-Process -Id {}; \"$($p.PeakWorkingSet64)|$($p.TotalProcessorTime.TotalMilliseconds)\"",
            std::process::id()
        );
        let Some(out) = powershell(&query) else { return ProcessProbe::default() };
        let mut parts = out.trim().split('|');
        ProcessProbe {
            peak_rss_bytes: parts.next().and_then(|v| v.parse().ok()),
            // PowerShell formats with the user's locale (a decimal comma in many of them).
            cpu_ms: parts.next().and_then(|v| v.replace(',', ".").parse().ok()),
        }
    } else {
        ProcessProbe::default()
    }
}

/// A coarse indicator of competing load when a suite starts: the 1-minute load average on Linux,
/// the CPU load percentage on Windows.
pub fn system_load() -> Option<String> {
    if cfg!(target_os = "linux") {
        let avg = std::fs::read_to_string("/proc/loadavg").ok()?;
        Some(format!("loadavg {}", avg.split_whitespace().take(3).collect::<Vec<_>>().join(" ")))
    } else if cfg!(windows) {
        // A single load sample says little on a shared machine; the number of compiler processes
        // running next to the benchmark says what the load was.
        let out = powershell(
            "$l = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average; \
             $c = @(Get-Process rustc,cargo -ErrorAction SilentlyContinue).Count; \"$l|$c\"",
        )?;
        let (load, compilers) = out.trim().split_once('|')?;
        Some(format!("cpu load {load}%, {compilers} rustc/cargo processes"))
    } else {
        None
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct HostInfo {
    pub os: String,
    pub arch: String,
    pub cpu_model: Option<String>,
    pub physical_cores: Option<u32>,
    pub logical_cpus: Option<u32>,
    pub memory_bytes: Option<u64>,
    /// Threads Rust (and therefore rayon) will use by default.
    pub available_parallelism: Option<usize>,
}

pub fn host_info() -> HostInfo {
    let mut info = HostInfo {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        available_parallelism: std::thread::available_parallelism().ok().map(usize::from),
        ..HostInfo::default()
    };
    if cfg!(target_os = "linux") {
        if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
            info.cpu_model = cpuinfo
                .lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_owned());
            info.logical_cpus = Some(cpuinfo.lines().filter(|l| l.starts_with("processor")).count() as u32);
            let mut cores: Vec<(&str, &str)> = Vec::new();
            let (mut physical, mut core) = ("", "");
            for line in cpuinfo.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    match k.trim() {
                        "physical id" => physical = v.trim(),
                        "core id" => core = v.trim(),
                        _ => {}
                    }
                }
                if line.is_empty() && !core.is_empty() {
                    cores.push((physical, core));
                }
            }
            cores.sort_unstable();
            cores.dedup();
            info.physical_cores = (!cores.is_empty()).then_some(cores.len() as u32);
        }
        info.memory_bytes = std::fs::read_to_string("/proc/meminfo").ok().and_then(|m| {
            let line = m.lines().find(|l| l.starts_with("MemTotal:"))?;
            line.split_whitespace().nth(1)?.parse::<u64>().ok().map(|kib| kib * 1024)
        });
        info.os = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|r| {
                let line = r.lines().find(|l| l.starts_with("PRETTY_NAME="))?;
                Some(line.trim_start_matches("PRETTY_NAME=").trim_matches('"').to_owned())
            })
            .unwrap_or(info.os);
    } else if cfg!(windows) {
        let query = "$p = Get-CimInstance Win32_Processor | Select-Object -First 1; \
                     $c = Get-CimInstance Win32_ComputerSystem; $o = Get-CimInstance Win32_OperatingSystem; \
                     \"$($p.Name.Trim())|$($p.NumberOfCores)|$($p.NumberOfLogicalProcessors)|$($c.TotalPhysicalMemory)|$($o.Caption) $($o.Version)\"";
        if let Some(out) = powershell(query) {
            let parts: Vec<&str> = out.trim().split('|').collect();
            if let [model, cores, logical, memory, os] = parts.as_slice() {
                info.cpu_model = Some((*model).to_owned());
                info.physical_cores = cores.parse().ok();
                info.logical_cpus = logical.parse().ok();
                info.memory_bytes = memory.parse().ok();
                info.os = (*os).to_owned();
            }
        }
    }
    info
}

fn powershell(command: &str) -> Option<String> {
    let output =
        Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", command]).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
