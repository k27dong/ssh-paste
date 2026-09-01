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
    run_with(ssh, &[], host, script)
}

pub fn run_with(ssh: &OsStr, extra_args: &[&str], host: &str, script: &str) -> Result<String> {
    let script = format!("sh -c {}", crate::sh::sh_quote(script)); // the remote login shell may be fish or csh, which cannot run these scripts
    let out = Command::new(ssh)
        .args(extra_args)
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
    let script = format!("sh -c {}", crate::sh::sh_quote(script)); // the remote login shell may be fish or csh, which cannot run these scripts
    let mut child = Command::new(ssh)
        .arg(host)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawning {}", ssh.to_string_lossy()))?;
    let mut stdin_handle = child.stdin.take().context("child stdin unavailable")?;
    let write_result = stdin_handle.write_all(stdin); // don't `?` yet: an early-exiting child's status must win over a broken-pipe write error
    drop(stdin_handle);
    let status = child.wait()?;
    if !status.success() {
        return Err(SshFailed(status.code().unwrap_or(1)).into());
    }
    write_result?;
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
    fn run_with_inserts_extra_args_before_the_host() {
        let sh = std::ffi::OsStr::new("sh");
        assert_eq!(
            run_with(sh, &[], "-c", "echo captured").unwrap(),
            run(sh, "-c", "echo captured").unwrap()
        );
        let argv = run_with(std::ffi::OsStr::new("echo"), &["A"], "B", "C").unwrap();
        assert_eq!(argv, "A B sh -c 'C'");
    }

    #[test]
    fn run_wraps_scripts_that_carry_quotes_and_expansions() {
        let sh = std::ffi::OsStr::new("sh");
        let home = std::env::var("HOME").unwrap();
        let out = run(sh, "-c", r#"printf '%s' "it's $HOME""#).unwrap();
        assert_eq!(out, format!("it's {home}"));
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

    #[test]
    fn stream_preserves_exit_code_when_child_exits_before_reading_stdin() {
        let sh = std::ffi::OsStr::new("sh");
        let payload = vec![0u8; 10 * 1024 * 1024];
        let err = stream(sh, "-c", "exit 9", &payload).unwrap_err();
        let failed = err.downcast_ref::<SshFailed>().unwrap();
        assert_eq!(failed.0, 9);
    }
}
