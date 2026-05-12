//! Auto-start at login via Task Scheduler. Uses the same `schtasks` recipe
//! documented in the README so the GUI toggle and the manual command produce
//! identical entries. Runs the RustCursor exe at logon with HIGHEST integrity
//! so no UAC prompt appears at startup.
//!
//! Create/delete go through `ShellExecuteEx` with the `runas` verb so a UAC
//! prompt appears when the parent is not elevated; query goes through plain
//! `Command` because /Query does not require elevation. Both paths suppress
//! the schtasks console window, which would otherwise flash when spawned from
//! a windows-subsystem parent.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::process::Command;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::PCWSTR;

const TASK_NAME: &str = "RustCursor";

/// `CREATE_NO_WINDOW`: suppresses the console allocation that would otherwise
/// flash a black box when spawning a console-subsystem child from a
/// windows-subsystem parent.
const NO_WINDOW: u32 = 0x0800_0000;

/// `WaitForSingleObject` infinite-timeout sentinel.
const INFINITE: u32 = 0xFFFF_FFFF;

/// Returns `true` if a scheduled task named `RustCursor` exists. We don't
/// validate its action, so a stale task pointing at an old exe path still
/// reports as enabled; the user can re-toggle to refresh.
pub fn is_enabled() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .creation_flags(NO_WINDOW)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

pub fn enable() -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("current_exe: {e}"))?
        .to_string_lossy()
        .into_owned();
    // Quote the exe path so spaces don't split the /TR argument.
    let args = format!(
        "/Create /TN {TASK_NAME} /SC ONLOGON /RL HIGHEST /TR \"\\\"{exe}\\\"\" /F"
    );
    run_elevated("schtasks.exe", &args)
}

pub fn disable() -> Result<(), String> {
    let args = format!("/Delete /TN {TASK_NAME} /F");
    run_elevated("schtasks.exe", &args)
}

/// Spawn `file` with `args` via `ShellExecuteEx` using the `runas` verb. When
/// the parent process is not elevated this triggers a UAC prompt; when it is
/// elevated the child inherits the token without a prompt. Waits for the
/// child to exit and returns `Err` on non-zero exit (including UAC cancel,
/// which surfaces as `ERROR_CANCELLED` before the child ever runs).
fn run_elevated(file: &str, args: &str) -> Result<(), String> {
    let verb_w: Vec<u16> = OsStr::new("runas").encode_wide().chain([0]).collect();
    let file_w: Vec<u16> = OsStr::new(file).encode_wide().chain([0]).collect();
    let args_w: Vec<u16> = OsStr::new(args).encode_wide().chain([0]).collect();

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb_w.as_ptr()),
        lpFile: PCWSTR(file_w.as_ptr()),
        lpParameters: PCWSTR(args_w.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };

    unsafe {
        ShellExecuteExW(&mut info).map_err(|e: windows::core::Error| {
            // ERROR_CANCELLED (1223) is what we get when the user clicks No on UAC.
            if (e.code().0 as u32) & 0xFFFF == 1223 {
                "Cancelled by user".to_string()
            } else {
                format!("ShellExecuteEx: {e}")
            }
        })?;

        if info.hProcess.is_invalid() {
            return Err("schtasks did not return a process handle".into());
        }

        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code: u32 = 0;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);

        if code == 0 {
            Ok(())
        } else {
            Err(format!("schtasks exited with code {code}"))
        }
    }
}
