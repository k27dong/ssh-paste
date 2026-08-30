use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use ssh_paste::clipboard::Payload;
use ssh_paste::config::Target;
use ssh_paste::{send, setup, shims, ssh};

static SPAWN_LOCK: Mutex<()> = Mutex::new(());

fn no_concurrent_spawns() -> MutexGuard<'static, ()> {
    SPAWN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn fake_ssh(dir: &Path) -> OsString {
    let path = dir.join("fake-ssh");
    fs::write(&path, "#!/bin/sh\nshift\nexec sh -c \"$1\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.into_os_string()
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ssh-paste-rt-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn target(dir: &Path) -> Target {
    Target {
        host: "testhost".into(),
        spool_dir: dir.join("spool").to_str().unwrap().into(),
        shim_dir: dir.join("bin").to_str().unwrap().into(),
    }
}

fn shim_read(dir: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(dir.join("bin").join("xclip"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn text_and_image_roundtrip_through_send_and_shims() {
    let _lock = no_concurrent_spawns();
    let dir = scratch("sendread");
    let ssh_bin = fake_ssh(&dir);
    let t = target(&dir);

    fs::create_dir_all(dir.join("bin")).unwrap();
    let spool_expr = ssh_paste::sh::remote_path_expr(&t.spool_dir);
    fs::write(dir.join("bin/xclip"), shims::render_xclip(&spool_expr)).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir.join("bin/xclip"), fs::Permissions::from_mode(0o755)).unwrap();

    send::send(&ssh_bin, "t", &t, Payload::Text("first text".into())).unwrap();
    let out = shim_read(&dir, &["-selection", "clipboard", "-t", "text/plain", "-o"]);
    assert_eq!(out.stdout, b"first text");

    let png = ssh_paste::clipboard::encode_png(1, 1, &[9, 9, 9, 255]).unwrap();
    send::send(&ssh_bin, "t", &t, Payload::Png(png.clone())).unwrap();
    let out = shim_read(&dir, &["-selection", "clipboard", "-t", "image/png", "-o"]);
    assert_eq!(out.stdout, png, "png bytes survive the pipeline exactly");
    let out = shim_read(&dir, &["-selection", "clipboard", "-t", "text/plain", "-o"]);
    assert_eq!(out.status.code(), Some(1), "text pruned after image send");

    send::send(&ssh_bin, "t", &t, Payload::Text("after image".into())).unwrap();
    let out = shim_read(&dir, &["-selection", "clipboard", "-t", "image/png", "-o"]);
    assert_eq!(out.status.code(), Some(1), "image pruned after text send");
}

#[test]
fn ssh_failure_surfaces_exit_code() {
    let _lock = no_concurrent_spawns();
    let dir = scratch("failure");
    let path = dir.join("failing-ssh");
    fs::write(&path, "#!/bin/sh\nexit 255\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

    let err = send::send(
        &path.into_os_string(),
        "t",
        &target(&dir),
        Payload::Text("x".into()),
    )
    .unwrap_err();
    assert_eq!(err.downcast_ref::<ssh::SshFailed>().unwrap().0, 255);
}

#[test]
fn setup_scripts_compose_end_to_end() {
    let _lock = no_concurrent_spawns();
    let dir = scratch("setupflow");
    let ssh_bin = fake_ssh(&dir);
    let t = target(&dir);
    let shim_dir_expr = ssh_paste::sh::remote_path_expr(&t.shim_dir);
    let spool_expr = ssh_paste::sh::remote_path_expr(&t.spool_dir);

    ssh::stream(
        &ssh_bin,
        &t.host,
        &setup::install_script(&shim_dir_expr, "xclip"),
        shims::render_xclip(&spool_expr).as_bytes(),
    )
    .unwrap();
    ssh::stream(
        &ssh_bin,
        &t.host,
        &setup::install_script(&shim_dir_expr, "wl-paste"),
        shims::render_wl_paste(&spool_expr).as_bytes(),
    )
    .unwrap();

    let report = ssh::run(&ssh_bin, &t.host, &setup::inspect_script(&shim_dir_expr)).unwrap();
    assert!(
        report.contains("SHIM:xclip") && report.contains("SHIM:wl-paste"),
        "{report}"
    );

    send::send(&ssh_bin, "t", &t, Payload::Text("probe".into())).unwrap();
    let read = ssh::run(
        &ssh_bin,
        &t.host,
        &format!("{shim_dir_expr}/xclip -selection clipboard -t text/plain -o"),
    )
    .unwrap();
    assert_eq!(read, "probe");

    ssh::run(
        &ssh_bin,
        &t.host,
        &setup::remove_script(&shim_dir_expr, &spool_expr),
    )
    .unwrap();
    assert!(!dir.join("bin/xclip").exists());
    assert!(!dir.join("spool").exists());
}
