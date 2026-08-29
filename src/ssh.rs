use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

#[derive(Debug)]
pub struct SshFailed(pub i32);

impl std::fmt::Display for SshFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ssh exited with status {}", self.0)
    }
}

impl std::error::Error for SshFailed {}

pub fn run(ssh: &OsStr, host: &str, script: &str) -> Result<String> {
    let out = Command::new(ssh)
        .arg(host)
        .arg(script)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("spawning {}", ssh.to_string_lossy()))?;
    if !out.status.success() {
        return Err(SshFailed(out.status.code().unwrap_or(1)).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn stream(ssh: &OsStr, host: &str, script: &str, stdin: &[u8]) -> Result<()> {
    let mut child = Command::new(ssh)
        .arg(host)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawning {}", ssh.to_string_lossy()))?;
    child
        .stdin
        .take()
        .context("child stdin unavailable")?
        .write_all(stdin)?;
    let status = child.wait()?;
    if !status.success() {
        return Err(SshFailed(status.code().unwrap_or(1)).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_captures_stdout_and_maps_exit_codes() {
        let sh = std::ffi::OsStr::new("sh");
        let out = run(sh, "-c", "echo captured").unwrap();
        assert_eq!(out, "captured");
        let err = run(sh, "-c", "exit 7").unwrap_err();
        let failed = err.downcast_ref::<SshFailed>().unwrap();
        assert_eq!(failed.0, 7);
    }

    #[test]
    fn stream_pipes_stdin() {
        let sh = std::ffi::OsStr::new("sh");
        let dir = std::env::temp_dir().join(format!("ssh-paste-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("out");
        stream(
            sh,
            "-c",
            &format!("cat > '{}'", dest.display()),
            b"bytes in flight",
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"bytes in flight");
    }
}
