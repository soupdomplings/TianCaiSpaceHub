//! Own the entire assistance process tree, including if Hub itself exits.
use anyhow::{Context, Result, ensure};
use tokio::process::Child;

/// A reused or inaccessible PID is treated as live. Cleanup never guesses that
/// another user's process can be terminated or that its files can be removed.
pub(crate) fn alive(pid: u32) -> Result<bool> {
    ensure!(pid > 0, "协助记录中的进程标识无效");
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError},
            System::Threading::{
                GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        };
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return Ok(GetLastError() != ERROR_INVALID_PARAMETER);
        }
        let mut code = 0;
        let result = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        Ok(result == 0 || code == 259)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(unsafe { libc::kill(pid.try_into()?, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = pid;
        Ok(true)
    }
}

#[cfg(windows)]
pub struct ProcessTree(std::os::windows::io::OwnedHandle);
#[cfg(windows)]
impl ProcessTree {
    pub fn attach(child: &Child) -> Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            ensure!(!raw.is_null(), "无法准备协助后台的退出保护");
            let job = std::os::windows::io::OwnedHandle::from_raw_handle(raw);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            ensure!(
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&info) as u32
                ) != 0,
                "无法启用协助后台退出保护"
            );
            ensure!(
                AssignProcessToJobObject(
                    job.as_raw_handle(),
                    child.raw_handle().context("无法核对协助后台进程")?
                ) != 0,
                "无法管理协助后台进程；未开始模型任务"
            );
            Ok(Self(job))
        }
    }
    pub async fn stop_and_confirm(&self) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            ensure!(
                TerminateJobObject(self.0.as_raw_handle(), 1) != 0,
                "协助进程树停止未确认"
            );
        }
        for _ in 0..50 {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            let result = unsafe {
                QueryInformationJobObject(
                    self.0.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&info) as u32,
                    std::ptr::null_mut(),
                )
            };
            ensure!(result != 0, "无法核对协助进程树退出状态");
            if info.ActiveProcesses == 0 {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        anyhow::bail!("协助子进程仍在退出，未清理记录")
    }
}
#[cfg(target_os = "macos")]
pub struct ProcessTree {
    group: i32,
    stopped: std::sync::atomic::AtomicBool,
}
#[cfg(target_os = "macos")]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        if !self.stopped.load(std::sync::atomic::Ordering::Relaxed) {
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
        }
    }
}
#[cfg(target_os = "macos")]
impl ProcessTree {
    pub fn attach(child: &Child) -> Result<Self> {
        let group: i32 = child.id().context("无法核对协助后台进程")?.try_into()?;
        ensure!(group > 0, "协助进程组标识无效");
        Ok(Self {
            group,
            stopped: std::sync::atomic::AtomicBool::new(false),
        })
    }
    pub async fn stop_and_confirm(&self) -> Result<()> {
        if self.stopped.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        unsafe {
            libc::kill(-self.group, libc::SIGKILL);
        }
        // No signal to any unrelated group. The group was created before exec.
        for _ in 0..50 {
            if unsafe { libc::kill(-self.group, 0) } < 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                self.stopped
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        anyhow::bail!("协助子进程退出尚未确认，未清理记录")
    }
}
#[cfg(not(any(windows, target_os = "macos")))]
pub struct ProcessTree;
#[cfg(not(any(windows, target_os = "macos")))]
impl ProcessTree {
    pub fn attach(_: &Child) -> Result<Self> {
        anyhow::bail!("本机协助仅支持 Windows / macOS")
    }
    pub async fn stop_and_confirm(&self) -> Result<()> {
        Ok(())
    }
}
