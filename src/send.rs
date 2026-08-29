use std::ffi::OsStr;

use anyhow::Result;

use crate::clipboard::Payload;
use crate::config::Target;
use crate::sh::remote_path_expr;
use crate::ssh;

pub fn receiver_script(spool_expr: &str, keep: &str, drop: &str) -> String {
    format!(
        "umask 077; d={spool_expr}; mkdir -p \"$d\" || exit 1; \
         if cat > \"$d\"/clip.tmp; then mv \"$d\"/clip.tmp \"$d\"/{keep} && rm -f \"$d\"/{drop}; \
         else rm -f \"$d\"/clip.tmp; exit 1; fi"
    )
}

pub fn send(ssh_bin: &OsStr, target_name: &str, target: &Target, payload: Payload) -> Result<()> {
    target.validate()?;
    let script = receiver_script(
        &remote_path_expr(&target.spool_dir),
        payload.keep_file(),
        payload.drop_file(),
    );
    let size = payload.bytes().len();
    ssh::stream(ssh_bin, &target.host, &script, payload.bytes())?;
    println!(
        "sent {} ({} bytes) to {}",
        payload.kind(),
        size,
        target_name
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_script_is_atomic_and_prunes_other_type() {
        let s = receiver_script("\"$HOME\"/'.cache/ssh-paste'", "clip.png", "clip.txt");
        assert!(s.contains("umask 077"));
        assert!(s.contains("clip.tmp"));
        let mv = s.find("mv ").unwrap();
        let cat = s.find("cat >").unwrap();
        assert!(cat < mv, "content lands in tmp before rename");
        assert!(s.contains("rm -f \"$d\"/clip.txt"));
    }

    #[test]
    fn receiver_script_executes_locally() {
        let dir = std::env::temp_dir().join(format!("ssh-paste-recv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = receiver_script(&format!("'{}'", dir.display()), "clip.txt", "clip.png");
        std::fs::write(dir.join("clip.png"), b"stale").unwrap();
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(b"payload").unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(std::fs::read(dir.join("clip.txt")).unwrap(), b"payload");
        assert!(!dir.join("clip.png").exists(), "other type pruned");
        assert!(!dir.join("clip.tmp").exists());
    }
}
