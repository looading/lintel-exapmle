#[cfg(target_os = "macos")]
mod macos {
    use std::{
        env,
        error::Error,
        fs::{self, File, OpenOptions},
        io::{self, Write},
        path::{Path, PathBuf},
        process::{Command, Output},
    };

    const LABEL: &str = "dev.lintel.world-clock.agent";
    const APP_ID: &str = "dev.lintel.world-clock";
    const NOT_FOUND: i32 = 113;

    type Result<T> = std::result::Result<T, Box<dyn Error>>;

    struct Agent {
        domain: String,
        target: String,
        plist: PathBuf,
        lock: PathBuf,
    }

    impl Agent {
        fn locate() -> Result<Self> {
            let uid = Command::new("/usr/bin/id").arg("-u").output()?;
            checked("读取当前用户", &uid)?;
            let uid: u32 = std::str::from_utf8(&uid.stdout)?.trim().parse()?;
            let domain = format!("gui/{uid}");
            let directories = directories::ProjectDirs::from("dev", "Lintel", "World Clock")
                .ok_or("无法找到当前用户的应用配置目录。")?;
            let home = directories::BaseDirs::new().ok_or("无法找到当前用户的主目录。")?;
            Ok(Self {
                target: format!("{domain}/{LABEL}"),
                domain,
                plist: home
                    .home_dir()
                    .join("Library/LaunchAgents")
                    .join(format!("{LABEL}.plist")),
                lock: directories.config_dir().join("background.lock"),
            })
        }

        fn lock(&self) -> Result<File> {
            if let Some(directory) = self.lock.parent() {
                fs::create_dir_all(directory)?;
            }
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&self.lock)?;
            lock.lock()?;
            Ok(lock)
        }

        fn is_loaded(&self) -> Result<bool> {
            let output = Command::new("/bin/launchctl")
                .args(["print", &self.target])
                .output()?;
            if output.status.code() == Some(NOT_FOUND) {
                return Ok(false);
            }
            checked("查询后台提醒", &output)?;
            Ok(true)
        }

        fn enable(&self) -> Result<bool> {
            let _lock = self.lock()?;
            if self.is_loaded()? {
                return Ok(true);
            }
            let executable = env::current_exe()?;
            bundled_executable(&executable)?;
            let content = agent_plist(&executable)?;
            let previous = match fs::read(&self.plist) {
                Ok(content) => Some(content),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            crate::write_config(&self.plist, |file| file.write_all(content.as_bytes()))?;
            let bootstrap = Command::new("/bin/launchctl")
                .args(["bootstrap", &self.domain])
                .arg(&self.plist)
                .output()
                .map_err(Into::into)
                .and_then(|output| checked("启动后台提醒", &output));
            if let Err(error) = bootstrap {
                if let Some(previous) = previous {
                    crate::write_config(&self.plist, |file| file.write_all(&previous))?;
                } else {
                    fs::remove_file(&self.plist)?;
                }
                return Err(error);
            }
            Ok(true)
        }

        fn disable(&self) -> Result<bool> {
            let _lock = self.lock()?;
            if self.is_loaded()? {
                let output = Command::new("/bin/launchctl")
                    .args(["bootout", &self.target])
                    .output()?;
                if !output.status.success() && self.is_loaded()? {
                    checked("关闭后台提醒", &output)?;
                }
            }
            match fs::remove_file(&self.plist) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            Ok(false)
        }
    }

    fn checked(action: &str, output: &Output) -> Result<()> {
        if output.status.success() {
            return Ok(());
        }
        let error = String::from_utf8_lossy(&output.stderr);
        let error = error.trim();
        if error.is_empty() {
            Err(format!("{action}失败（{}）。", output.status).into())
        } else {
            Err(format!("{action}失败。{error}").into())
        }
    }

    fn bundled_executable(path: &Path) -> Result<()> {
        let mut parents = path.ancestors().skip(1);
        let valid = parents.next().and_then(Path::file_name) == Some("MacOS".as_ref())
            && parents.next().and_then(Path::file_name) == Some("Contents".as_ref())
            && parents.next().and_then(Path::extension) == Some("app".as_ref());
        if valid {
            Ok(())
        } else {
            Err("请从一个固定位置打开世界时钟.app，再启用后台提醒。".into())
        }
    }

    fn agent_plist(executable: &Path) -> Result<String> {
        let executable = executable.to_str().ok_or("应用路径必须是 UTF-8。")?;
        if executable.chars().any(char::is_control) {
            return Err("应用路径包含控制字符。".into());
        }
        let executable = executable
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>AssociatedBundleIdentifiers</key>
    <array>
        <string>{APP_ID}</string>
    </array>
    <key>ProgramArguments</key>
    <array>
        <string>{executable}</string>
        <string>--clock-agent</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
</dict>
</plist>
"#
        ))
    }

    pub fn status() -> std::result::Result<bool, String> {
        Agent::locate()
            .and_then(|agent| agent.is_loaded())
            .map_err(|error| error.to_string())
    }

    pub fn enable() -> std::result::Result<bool, String> {
        Agent::locate()
            .and_then(|agent| agent.enable())
            .map_err(|error| error.to_string())
    }

    pub fn disable() -> std::result::Result<bool, String> {
        Agent::locate()
            .and_then(|agent| agent.disable())
            .map_err(|error| error.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn plist_preserves_bundled_executable_path_as_one_argument() {
            let executable = Path::new("/Applications/世界 & 时钟.app/Contents/MacOS/clock");
            bundled_executable(executable).unwrap();
            let plist = tempfile::NamedTempFile::new().unwrap();
            fs::write(plist.path(), agent_plist(executable).unwrap()).unwrap();
            let output = Command::new("/usr/bin/plutil")
                .args(["-extract", "ProgramArguments.0", "raw", "-o", "-", "--"])
                .arg(plist.path())
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim(),
                executable.to_str().unwrap()
            );
        }

        #[test]
        fn rejects_a_standalone_development_binary() {
            assert!(bundled_executable(Path::new("/project/target/debug/clock")).is_err());
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{disable, enable, status};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{agent_registered, claim_agent, disable, enable, status};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn status() -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn enable() -> Result<bool, String> {
    Err("后台提醒支持 macOS 和 Windows。".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn disable() -> Result<bool, String> {
    Ok(false)
}
