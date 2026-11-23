use std::{
    env,
    fs::{self, File, OpenOptions},
    io,
    mem::size_of_val,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr, thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::{
        Registry::{
            HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
            RegSetKeyValueW,
        },
        Threading::CREATE_NO_WINDOW,
    },
};

// https://learn.microsoft.com/windows/win32/setupapi/run-and-runonce-registry-keys
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "dev.lintel.world-clock.agent";
const MAX_RUN_COMMAND: usize = 260;
const STARTUP_ARGS: [&str; 2] = ["--clock-agent", "--windows-startup"];

pub fn status() -> Result<bool, String> {
    let result = (|| {
        let registered = process_registered()?;
        Ok::<_, io::Error>(registered && Agent::new()?.is_running()?)
    })();
    result.map_err(|error| format!("查询后台提醒失败。{error}"))
}

pub fn enable() -> Result<bool, String> {
    Agent::new()
        .and_then(|agent| agent.enable())
        .map_err(|error| format!("启动后台提醒失败。{error}"))
}

pub fn disable() -> Result<bool, String> {
    Agent::new()
        .and_then(|agent| agent.disable())
        .map_err(|error| format!("关闭后台提醒失败。{error}"))
}

/// The managed agent checks this before doing work. Removing the startup
/// registration also stops the current worker without terminating another PID.
pub fn agent_registered() -> bool {
    process_registered().unwrap_or(false)
}

/// Gives a startup worker an independent, process-lifetime lease. A second
/// instance (for example, if login startup races an immediate launch) exits.
pub fn claim_agent() -> io::Result<Option<File>> {
    if !process_registered()? {
        return Ok(None);
    }

    let file = open_lock(&Agent::new()?.ownership_path)?;
    match file.try_lock() {
        Ok(()) if process_registered()? => Ok(Some(file)),
        Ok(()) | Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => Err(error),
    }
}

struct Agent {
    ownership_path: PathBuf,
    management_path: PathBuf,
}

impl Agent {
    fn new() -> io::Result<Self> {
        let clock = crate::config_path("clock.json")
            .ok_or_else(|| io::Error::other("无法找到当前用户的应用配置目录。"))?;
        Ok(Self {
            ownership_path: clock.with_file_name("windows-background-worker.lock"),
            management_path: clock.with_file_name("background.lock"),
        })
    }

    fn is_running(&self) -> io::Result<bool> {
        // This lock is only owned by a managed worker. The foreground GUI
        // briefly owns the separate scheduler lock while checking deadlines.
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.ownership_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        match file.try_lock() {
            Ok(()) => Ok(false),
            Err(fs::TryLockError::WouldBlock) => Ok(true),
            Err(fs::TryLockError::Error(error)) => Err(error),
        }
    }

    fn lock_management(&self) -> io::Result<File> {
        let file = open_lock(&self.management_path)?;
        file.lock()?;
        Ok(file)
    }

    fn enable(&self) -> io::Result<bool> {
        let _guard = self.lock_management()?;
        let executable = env::current_exe()?;
        let command = startup_command(&executable)?;
        let previous = read_registration()?;
        let changed = previous.as_deref() != Some(command.as_slice());

        if !changed && self.is_running()? {
            return Ok(true);
        }

        let started = (|| {
            if changed {
                if self.is_running()? {
                    // An older copy (or an agent still stopping after disable)
                    // must see the removed entry before new registration.
                    write_registration(None)?;
                    self.wait_for_worker_exit()?;
                }
                write_registration(Some(&command))?;
            }
            self.start_worker(&executable)
        })();
        if let Err(error) = started {
            if changed {
                write_registration(previous.as_deref()).map_err(|rollback| {
                    io::Error::other(format!("{error} 无法恢复之前的登录启动项。{rollback}"))
                })?;
            }
            return Err(error);
        }

        Ok(true)
    }

    fn wait_for_worker_exit(&self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.is_running()? {
            if Instant::now() >= deadline {
                return Err(io::Error::other("上一个后台提醒进程尚未退出。"));
            }
            thread::sleep(Duration::from_millis(40));
        }
        Ok(())
    }

    fn start_worker(&self, executable: &Path) -> io::Result<()> {
        let mut child = Command::new(executable)
            .args(STARTUP_ARGS)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(6);
        let result = loop {
            match self.is_running() {
                Ok(true) => break Ok(()),
                Ok(false) if Instant::now() < deadline => {}
                Ok(false) => break Err(io::Error::other("等待后台提醒服务启动超时。")),
                Err(error) => break Err(error),
            }
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    break Err(io::Error::other(format!(
                        "后台提醒进程已退出（{status}）。"
                    )));
                }
                Err(error) => break Err(error),
            }
            thread::sleep(Duration::from_millis(40));
        };
        if result.is_err() {
            // Only clean up the process handle spawned by this attempt.
            let _ = child.kill();
            let _ = child.wait();
        }
        result
    }

    fn disable(&self) -> io::Result<bool> {
        let _guard = self.lock_management()?;
        write_registration(None)?;
        self.wait_for_worker_exit()?;
        Ok(false)
    }
}

fn open_lock(path: &Path) -> io::Result<File> {
    let directory = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(directory)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn process_registered() -> io::Result<bool> {
    let Some(saved) = read_registration()? else {
        return Ok(false);
    };
    Ok(saved == startup_command(&env::current_exe()?)?)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn startup_command(executable: &Path) -> io::Result<Vec<u16>> {
    // Win32 command lines use quotes to keep paths with spaces as a single
    // argument. An executable path ends in a filename, never a backslash.
    let path: Vec<_> = executable.as_os_str().encode_wide().collect();
    if path
        .iter()
        .any(|&unit| unit == 0 || unit == u16::from(b'"'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "应用程序路径无效。",
        ));
    }
    let mut command = vec![u16::from(b'"')];
    command.extend(path);
    command.extend([u16::from(b'"'), u16::from(b' ')]);
    command.extend(STARTUP_ARGS.join(" ").encode_utf16());
    if command.len() > MAX_RUN_COMMAND {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "应用路径过长，不能注册登录启动。请把应用放到更短的固定路径下。",
        ));
    }
    command.push(0);
    Ok(command)
}

fn read_registration() -> io::Result<Option<Vec<u16>>> {
    let key = wide(RUN_KEY);
    let name = wide(RUN_VALUE);
    let mut command = vec![0u16; MAX_RUN_COMMAND + 1];
    let mut bytes = size_of_val(command.as_slice()) as u32;
    // SAFETY: key and name are NUL-terminated and command is writable for the
    // byte count supplied; the Win32 call does not retain these pointers.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            command.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    match status {
        ERROR_SUCCESS => {
            command.truncate(bytes as usize / size_of::<u16>());
            Ok(Some(command))
        }
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
        error => Err(io::Error::from_raw_os_error(error as i32)),
    }
}

fn write_registration(command: Option<&[u16]>) -> io::Result<()> {
    let key = wide(RUN_KEY);
    let name = wide(RUN_VALUE);
    // SAFETY: all strings are NUL-terminated, and data is readable for its
    // exact byte count. The Windows registry copies data synchronously.
    let status = unsafe {
        if let Some(command) = command {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                command.as_ptr().cast(),
                size_of_val(command) as u32,
            )
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr())
        }
    };
    match status {
        ERROR_SUCCESS => Ok(()),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND if command.is_none() => Ok(()),
        error => Err(io::Error::from_raw_os_error(error as i32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_preserves_paths_with_spaces_and_unicode() {
        let command = startup_command(Path::new(r"C:\Clock 下载\世界时钟.exe")).unwrap();
        assert_eq!(
            String::from_utf16(&command[..command.len() - 1]).unwrap(),
            r#""C:\Clock 下载\世界时钟.exe" --clock-agent --windows-startup"#
        );
        assert_eq!(command.last(), Some(&0));
    }

    #[test]
    fn startup_rejects_commands_exceeding_documented_length() {
        let executable = format!("C:\\{}.exe", "a".repeat(MAX_RUN_COMMAND));
        let error = startup_command(Path::new(&executable)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
