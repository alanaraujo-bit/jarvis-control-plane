//! Native machine metrics for the compact live-session HUD.
//!
//! The webview can see its JavaScript heap, but that is not the memory people
//! mean when they ask how much of the machine the app is using. Read the OS
//! counters directly and return bytes; formatting belongs to the surface.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMetrics {
    /// Resident working set of the J.A.R.V.I.S. desktop process.
    pub process_memory_bytes: Option<u64>,
    pub system_memory_used_bytes: Option<u64>,
    pub system_memory_total_bytes: Option<u64>,
}

#[tauri::command]
pub fn system_metrics() -> SystemMetrics {
    read_metrics()
}

#[cfg(windows)]
fn read_metrics() -> SystemMetrics {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    // SAFETY: every API receives its documented, correctly-sized structure;
    // every real handle opened here is closed before returning.
    unsafe {
        let root_pid = GetCurrentProcessId();
        let mut process_bytes = working_set(root_pid);

        // WebView2 renders in child processes. Counting jarvis.exe alone
        // omits most of the interface and produces a pleasantly small but
        // false number. Sum its WebView2 descendants while deliberately
        // excluding agent/PTY descendants: those are the workload, not the
        // desktop shell labelled "J.A.R.V.I.S. RAM" in the HUD.
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot != INVALID_HANDLE_VALUE {
            let mut entries = Vec::<(u32, u32, String)>::new();
            let mut entry: PROCESSENTRY32W = zeroed();
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
            while ok {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(entry.szExeFile.len());
                entries.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase(),
                ));
                ok = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);

            let mut descendants = vec![root_pid];
            loop {
                let before = descendants.len();
                for (pid, parent, _) in &entries {
                    if descendants.contains(parent) && !descendants.contains(pid) {
                        descendants.push(*pid);
                    }
                }
                if descendants.len() == before {
                    break;
                }
            }

            for (pid, _, executable) in entries {
                if descendants.contains(&pid) && executable == "msedgewebview2.exe" {
                    if let Some(bytes) = working_set(pid) {
                        process_bytes = Some(process_bytes.unwrap_or(0).saturating_add(bytes));
                    }
                }
            }
        }

        let mut system: MEMORYSTATUSEX = zeroed();
        system.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        let system_ok = GlobalMemoryStatusEx(&mut system) != 0;

        return SystemMetrics {
            process_memory_bytes: process_bytes,
            system_memory_used_bytes: system_ok
                .then_some(system.ullTotalPhys.saturating_sub(system.ullAvailPhys)),
            system_memory_total_bytes: system_ok.then_some(system.ullTotalPhys),
        };
    }

    unsafe fn working_set(pid: u32) -> Option<u64> {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut counters: PROCESS_MEMORY_COUNTERS = zeroed();
        counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = GetProcessMemoryInfo(handle, &mut counters, counters.cb) != 0;
        CloseHandle(handle);
        ok.then_some(counters.WorkingSetSize as u64)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn native_memory_counters_are_sane() {
        let metrics = read_metrics();
        let process = metrics
            .process_memory_bytes
            .expect("this test process has a working set");
        let used = metrics
            .system_memory_used_bytes
            .expect("Windows reports used physical RAM");
        let total = metrics
            .system_memory_total_bytes
            .expect("Windows reports total physical RAM");

        assert!(process > 0);
        assert!(used > 0 && used <= total);
    }
}

// Keep the command portable without presenting a platform-specific guess as
// a real measurement. The current native product target is Windows.
#[cfg(not(windows))]
fn read_metrics() -> SystemMetrics {
    SystemMetrics {
        process_memory_bytes: None,
        system_memory_used_bytes: None,
        system_memory_total_bytes: None,
    }
}
