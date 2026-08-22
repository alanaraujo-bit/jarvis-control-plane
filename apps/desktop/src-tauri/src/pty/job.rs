//! Windows job objects — process-tree containment.
//!
//! A ConPTY child does not take its descendants with it when it dies, and agent
//! CLIs spawn plenty: `node`, `git`, nested shells, language servers. Killing
//! only the direct child leaves those running, so closing a terminal tab would
//! quietly leak processes for the rest of the session.
//!
//! Every PTY child is assigned to a job object created with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Two things follow:
//!
//! 1. Dropping the job terminates the whole tree — closing a tab is clean.
//! 2. If J.A.R.V.I.S. is killed outright, the handle closes with the process
//!    and Windows terminates the tree anyway. No orphans, even after a crash.

#[cfg(windows)]
mod imp {
    use std::io;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JobObjectExtendedLimitInformation,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// An owned job object. Dropping it terminates every process inside.
    pub struct ProcessJob {
        handle: HANDLE,
    }

    // The handle is only used through thread-safe Win32 calls.
    unsafe impl Send for ProcessJob {}
    unsafe impl Sync for ProcessJob {}

    impl ProcessJob {
        pub fn new() -> io::Result<Self> {
            // SAFETY: a null name creates an anonymous job; the returned handle
            // is owned here and closed exactly once, in Drop.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            // SAFETY: `info` matches the class being set and outlives the call.
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                let err = io::Error::last_os_error();
                unsafe { CloseHandle(handle) };
                return Err(err);
            }

            Ok(Self { handle })
        }

        /// Put a process, and therefore everything it goes on to spawn, in the job.
        pub fn assign(&self, pid: u32) -> io::Result<()> {
            // SAFETY: pid is validated by OpenProcess; the handle is closed below.
            let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            let ok = unsafe { AssignProcessToJobObject(self.handle, process) };
            let err = io::Error::last_os_error();
            unsafe { CloseHandle(process) };

            if ok == 0 {
                return Err(err);
            }
            Ok(())
        }
    }

    impl Drop for ProcessJob {
        fn drop(&mut self) {
            // Closing the last handle triggers KILL_ON_JOB_CLOSE.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;

    /// No-op outside Windows. Process-group containment on Unix is handled by
    /// the PTY session leader, so nothing extra is required here.
    pub struct ProcessJob;

    impl ProcessJob {
        pub fn new() -> io::Result<Self> {
            Ok(Self)
        }
        pub fn assign(&self, _pid: u32) -> io::Result<()> {
            Ok(())
        }
    }
}

pub use imp::ProcessJob;

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    /// Proves the containment guarantee against real processes: a child placed
    /// in the job must be gone once the job is dropped.
    #[test]
    fn dropping_the_job_terminates_its_processes() {
        let job = ProcessJob::new().expect("create job");

        let mut child = Command::new("cmd")
            .args(["/c", "ping", "-n", "30", "127.0.0.1"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .expect("spawn");

        job.assign(child.id()).expect("assign to job");
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "child should still be running before the job is dropped"
        );

        drop(job);

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().expect("try_wait").is_some() {
                return; // terminated, as required
            }
            assert!(Instant::now() < deadline, "job drop did not kill the child");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
