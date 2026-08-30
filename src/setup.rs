use std::ffi::OsStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::clipboard::Payload;
use crate::config::{Config, Target, default_shim_dir, default_spool_dir};
use crate::sh::{remote_path_expr, sh_quote};
use crate::shims::{MARKER_PREFIX, render_wl_paste, render_xclip};
use crate::{send, ssh};

pub fn inspect_script(shim_dir_expr: &str) -> String {
    let marker = sh_quote(MARKER_PREFIX);
    format!(
        r#"dir={shim_dir_expr}
for name in xclip wl-paste; do
  p=$(command -v "$name" 2>/dev/null || true)
  if [ -n "$p" ] && ! head -n 2 "$p" 2>/dev/null | grep -q {marker}; then
    echo "REAL:$name:$p"
  fi
  f="$dir/$name"
  if [ -e "$f" ]; then
    if head -n 2 "$f" 2>/dev/null | grep -q {marker}; then echo "SHIM:$name"; else echo "UNMARKED:$name"; fi
  fi
done
case ":$PATH:" in *":$dir:"*) echo PATHOK ;; *) echo NOPATH ;; esac"#
    )
}

pub fn install_script(shim_dir_expr: &str, name: &str) -> String {
    let name = sh_quote(name);
    format!(
        "dir={shim_dir_expr}; mkdir -p \"$dir\" || exit 1; \
         if cat > \"$dir\"/{name}.tmp; then chmod 755 \"$dir\"/{name}.tmp && mv \"$dir\"/{name}.tmp \"$dir\"/{name}; \
         else rm -f \"$dir\"/{name}.tmp; exit 1; fi"
    )
}

pub fn remove_script(shim_dir_expr: &str, spool_expr: &str) -> String {
    let marker = sh_quote(MARKER_PREFIX);
    format!(
        r#"dir={shim_dir_expr}
for name in xclip wl-paste; do
  f="$dir/$name"
  if [ -e "$f" ] && head -n 2 "$f" 2>/dev/null | grep -q {marker}; then rm -f "$f"; fi
done
rm -f {spool_expr}/clip.png {spool_expr}/clip.txt {spool_expr}/clip.tmp
rmdir {spool_expr} 2>/dev/null || true"#
    )
}

pub fn resolution_script(shim_dir_expr: &str) -> String {
    format!(
        r#"dir={shim_dir_expr}
for name in xclip wl-paste; do
  echo "RESOLVED:$name:$(command -v "$name" 2>/dev/null)"
  echo "EXPECTED:$name:$dir/$name"
done"#
    )
}

pub fn setup(
    ssh_bin: &OsStr,
    cfg: &mut Config,
    ssh_alias: &str,
    name: Option<&str>,
    force: bool,
) -> Result<()> {
    let target_name = name.unwrap_or(ssh_alias);
    let target = match cfg.targets.get(target_name) {
        Some(existing) => Target {
            host: ssh_alias.to_string(),
            spool_dir: existing.spool_dir.clone(),
            shim_dir: existing.shim_dir.clone(),
        },
        None => Target {
            host: ssh_alias.to_string(),
            spool_dir: default_spool_dir(),
            shim_dir: default_shim_dir(),
        },
    };
    target.validate()?;

    let shim_dir_expr = remote_path_expr(&target.shim_dir);
    let spool_expr = remote_path_expr(&target.spool_dir);

    ssh::run(ssh_bin, &target.host, "true").with_context(|| {
        format!("cannot reach {ssh_alias} over ssh; make sure `ssh {ssh_alias}` connects first")
    })?;

    let inspected = ssh::run(ssh_bin, &target.host, &inspect_script(&shim_dir_expr))
        .with_context(|| format!("inspecting {ssh_alias} for clipboard tools"))?;

    let mut path_ok = false;
    let mut forced_warnings = Vec::new();
    for line in inspected.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("REAL:") {
            let (tool, path) = rest.split_once(':').unwrap_or((rest, "an unknown path"));
            if !force {
                bail!(
                    "a real {tool} exists on {ssh_alias} at {path}; ssh-paste shims would shadow a working clipboard tool. Re-run with --force if that is what you want"
                );
            }
            forced_warnings.push(format!(
                "warning: shadowing real {tool} at {path} (--force)"
            ));
        } else if let Some(tool) = line.strip_prefix("UNMARKED:") {
            if !force {
                bail!(
                    "{}/{tool} on {ssh_alias} was not written by ssh-paste; refusing to overwrite a file that is not ours. Re-run with --force to replace it",
                    target.shim_dir
                );
            }
            forced_warnings.push(format!(
                "warning: overwriting foreign file at {}/{tool} (--force)",
                target.shim_dir
            ));
        } else if line == "PATHOK" {
            path_ok = true;
        }
    }
    if !path_ok {
        bail!(
            "{} is not on PATH for non-interactive ssh on {ssh_alias}, so the shims would never be found. Add it to PATH in the startup file ssh reads (~/.profile or ~/.zshenv) and re-run `ssh-paste setup {ssh_alias}`",
            target.shim_dir
        );
    }
    for warning in &forced_warnings {
        eprintln!("{warning}");
    }

    for (tool, body) in [
        ("xclip", render_xclip(&spool_expr)),
        ("wl-paste", render_wl_paste(&spool_expr)),
    ] {
        ssh::stream(
            ssh_bin,
            &target.host,
            &install_script(&shim_dir_expr, tool),
            body.as_bytes(),
        )
        .with_context(|| {
            format!(
                "installing the {tool} shim into {} on {ssh_alias}",
                target.shim_dir
            )
        })?;
    }

    let resolution = ssh::run(ssh_bin, &target.host, &resolution_script(&shim_dir_expr))
        .with_context(|| format!("checking which xclip and wl-paste {ssh_alias} resolves"))?;
    let field = |prefix: &str, tool: &str| {
        resolution.lines().map(str::trim).find_map(|line| {
            line.strip_prefix(prefix)?
                .strip_prefix(tool)?
                .strip_prefix(':')
        })
    };
    for tool in ["xclip", "wl-paste"] {
        let (Some(resolved), Some(expected)) = (field("RESOLVED:", tool), field("EXPECTED:", tool))
        else {
            continue;
        };
        if resolved != expected {
            eprintln!(
                "warning: {tool} on {ssh_alias} resolves to {}, not the shim at {expected}; Ctrl+V will keep hitting that binary until {} comes first on PATH",
                if resolved.is_empty() {
                    "nothing on PATH"
                } else {
                    resolved
                },
                target.shim_dir
            );
        }
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the system clock is set before the unix epoch")?
        .as_nanos();
    let probe = format!("ssh-paste probe {nonce}");
    send::send(ssh_bin, target_name, &target, Payload::Text(probe.clone()))?;
    let readback = ssh::run(
        ssh_bin,
        &target.host,
        &format!("{shim_dir_expr}/xclip -selection clipboard -t text/plain -o"),
    )
    .with_context(|| {
        format!(
            "reading the probe back through {}/xclip on {ssh_alias}",
            target.shim_dir
        )
    })?;
    ssh::run(
        ssh_bin,
        &target.host,
        &format!("rm -f {spool_expr}/clip.txt {spool_expr}/clip.png"),
    )
    .with_context(|| {
        format!(
            "clearing the probe out of {} on {ssh_alias}",
            target.spool_dir
        )
    })?;
    if readback != probe {
        bail!(
            "the shim on {ssh_alias} read back '{readback}' instead of '{probe}', so the clipboard round-trip is broken and the target was not saved. Check that {} is writable and that {}/xclip runs there",
            target.spool_dir,
            target.shim_dir
        );
    }

    let shim_dir = target.shim_dir.clone();
    cfg.targets.insert(target_name.to_string(), target);
    let made_default = cfg.default_target.is_none();
    if made_default {
        cfg.default_target = Some(target_name.to_string());
    }
    cfg.save()?;

    println!("installed xclip and wl-paste in {shim_dir} on {ssh_alias}");
    println!("probe ok: clipboard round-trip verified through the shim");
    println!(
        "target '{target_name}' saved{}",
        if made_default {
            " and set as the default"
        } else {
            ""
        }
    );
    Ok(())
}

pub fn remove(ssh_bin: &OsStr, cfg: &mut Config, name: &str) -> Result<()> {
    let (resolved, target) = cfg.resolve(Some(name))?;
    let resolved = resolved.to_string();
    target.validate()?;
    let host = target.host.clone();
    let shim_dir = target.shim_dir.clone();
    let spool_dir = target.spool_dir.clone();
    let script = remove_script(
        &remote_path_expr(&target.shim_dir),
        &remote_path_expr(&target.spool_dir),
    );

    ssh::run(ssh_bin, &host, &script)
        .with_context(|| format!("removing the ssh-paste shims and spool from {host}"))?;

    cfg.targets.remove(&resolved);
    if cfg.default_target.as_deref() == Some(resolved.as_str()) {
        cfg.default_target = None;
    }
    cfg.save()?;

    println!("removed the ssh-paste shims from {shim_dir} and the spool {spool_dir} on {host}");
    println!("target '{resolved}' forgotten");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_script_reports_all_states() {
        let dir = std::env::temp_dir().join(format!("ssh-paste-inspect-{}", std::process::id()));
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("xclip"),
            "#!/bin/sh\n# generated by ssh-paste v0.0.0\n",
        )
        .unwrap();
        std::fs::write(bin.join("wl-paste"), "#!/bin/sh\nreal tool\n").unwrap();
        let script = inspect_script(&format!("'{}'", bin.display()));
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("PATH='{}':\"$PATH\"; {script}", bin.display()))
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("SHIM:xclip"), "{text}");
        assert!(
            text.contains("UNMARKED:wl-paste") || text.contains("REAL:wl-paste"),
            "{text}"
        );
        assert!(text.contains("PATHOK"), "{text}");
    }

    #[test]
    fn inspect_script_expands_home_on_the_remote_side() {
        let home = std::env::temp_dir().join(format!("ssh-paste-home-{}", std::process::id()));
        let bin = home.join(".local").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("xclip"),
            "#!/bin/sh\n# generated by ssh-paste v0.0.0\n",
        )
        .unwrap();
        let script = inspect_script(&remote_path_expr("~/.local/bin"));
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .env("HOME", &home)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("SHIM:xclip"), "{text}");
        assert!(text.contains("PATHOK"), "{text}");
    }

    #[test]
    fn resolution_script_exposes_a_shadowed_shim() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ssh-paste-resolve-{}", std::process::id()));
        let shim_dir = dir.join("bin");
        let real_dir = dir.join("realbin");
        for d in [&shim_dir, &real_dir] {
            std::fs::create_dir_all(d).unwrap();
            let tool = d.join("xclip");
            std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let script = resolution_script(&format!("'{}'", shim_dir.display()));
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    real_dir.display(),
                    shim_dir.display()
                ),
            )
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(
            text.contains(&format!("RESOLVED:xclip:{}/xclip", real_dir.display())),
            "{text}"
        );
        assert!(
            text.contains(&format!("EXPECTED:xclip:{}/xclip", shim_dir.display())),
            "{text}"
        );
    }

    #[test]
    fn install_script_writes_executable_atomically() {
        let dir = std::env::temp_dir().join(format!("ssh-paste-install-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = install_script(&format!("'{}'", dir.display()), "xclip");
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"#!/bin/sh\nexit 0\n")
            .unwrap();
        assert!(child.wait().unwrap().success());
        let meta = std::fs::metadata(dir.join("xclip")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
    }

    #[test]
    fn remove_script_only_deletes_marked_files() {
        let dir = std::env::temp_dir().join(format!("ssh-paste-remove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        let spool = dir.join("spool");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&spool).unwrap();
        std::fs::write(
            bin.join("xclip"),
            "#!/bin/sh\n# generated by ssh-paste v0.0.0\n",
        )
        .unwrap();
        std::fs::write(bin.join("wl-paste"), "#!/bin/sh\nreal tool\n").unwrap();
        std::fs::write(spool.join("clip.txt"), "x").unwrap();
        std::fs::write(spool.join("clip.png"), "x").unwrap();
        std::fs::write(spool.join("notes.txt"), "not ours").unwrap();
        let script = remove_script(
            &format!("'{}'", bin.display()),
            &format!("'{}'", spool.display()),
        );
        let ok = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .status()
            .unwrap();
        assert!(ok.success());
        assert!(!bin.join("xclip").exists(), "marked shim deleted");
        assert!(bin.join("wl-paste").exists(), "foreign file preserved");
        assert!(!spool.join("clip.txt").exists(), "spooled text deleted");
        assert!(!spool.join("clip.png").exists(), "spooled image deleted");
        assert!(spool.join("notes.txt").exists(), "foreign spool file kept");
        assert!(spool.exists(), "spool with foreign content kept");
    }

    #[test]
    fn remove_script_drops_a_spool_holding_only_our_files() {
        let dir =
            std::env::temp_dir().join(format!("ssh-paste-remove-ours-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let spool = dir.join("spool");
        std::fs::create_dir_all(&spool).unwrap();
        for name in ["clip.txt", "clip.png", "clip.tmp"] {
            std::fs::write(spool.join(name), "x").unwrap();
        }
        let script = remove_script(
            &format!("'{}'", dir.join("bin").display()),
            &format!("'{}'", spool.display()),
        );
        let ok = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .status()
            .unwrap();
        assert!(ok.success());
        assert!(!spool.exists(), "emptied spool removed");
    }

    #[test]
    fn remove_script_spares_a_home_shaped_spool_dir() {
        let home =
            std::env::temp_dir().join(format!("ssh-paste-remove-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Documents")).unwrap();
        std::fs::write(home.join("Documents").join("keep.txt"), "precious").unwrap();
        let script = remove_script(&remote_path_expr("~/.local/bin"), &remote_path_expr("~/"));
        let ok = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .env("HOME", &home)
            .status()
            .unwrap();
        assert!(ok.success());
        assert!(
            home.join("Documents").join("keep.txt").exists(),
            "a spool_dir of ~/ must not take the home directory with it"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
