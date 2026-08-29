use std::fs;
use std::process::{Command, Output};

use ssh_paste::shims;

const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nfakebody";
const TEXT: &str = "hello from mac";

enum Spool {
    Image,
    Text,
    Empty,
}

fn run_shim(render: fn(&str) -> String, spool: Spool, args: &[&str]) -> Output {
    let dir = tempdir();
    match spool {
        Spool::Image => fs::write(dir.join("clip.png"), PNG_BYTES).unwrap(),
        Spool::Text => fs::write(dir.join("clip.txt"), TEXT).unwrap(),
        Spool::Empty => {}
    }
    let script = render(&format!("'{}'", dir.display()));
    let shim = dir.join("shim.sh");
    fs::write(&shim, script).unwrap();
    let mut cmd = Command::new("sh");
    cmd.arg(&shim).args(args);
    cmd.output().unwrap()
}

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ssh-paste-shim-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap()
}

#[test]
fn xclip_targets_row() {
    let out = run_shim(
        shims::render_xclip,
        Spool::Image,
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
    );
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("image/png"),
        "claude code greps image/(png|...): {stdout}"
    );

    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
    );
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("text/plain") && !stdout.contains("image/png"));

    let out = run_shim(
        shims::render_xclip,
        Spool::Empty,
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
    );
    assert_eq!(code(&out), 1);
}

#[test]
fn xclip_image_read_rows() {
    let out = run_shim(
        shims::render_xclip,
        Spool::Image,
        &["-selection", "clipboard", "-t", "image/png", "-o"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, PNG_BYTES);

    for spool in [Spool::Text, Spool::Empty] {
        let out = run_shim(
            shims::render_xclip,
            spool,
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        );
        assert_eq!(code(&out), 1);
        assert!(out.stdout.is_empty());
    }

    let out = run_shim(
        shims::render_xclip,
        Spool::Image,
        &["-selection", "clipboard", "-t", "image/bmp", "-o"],
    );
    assert_eq!(code(&out), 1, "only png is stored; bmp must be unavailable");
}

#[test]
fn xclip_text_read_rows() {
    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        &["-selection", "clipboard", "-t", "text/plain", "-o"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes(), "no added trailing newline");

    let out = run_shim(shims::render_xclip, Spool::Text, &["-o"]);
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes());

    let out = run_shim(
        shims::render_xclip,
        Spool::Image,
        &["-selection", "clipboard", "-t", "text/plain", "-o"],
    );
    assert_eq!(code(&out), 1);
}

#[test]
fn xclip_rejects_unsupported_loudly() {
    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        &["-selection", "clipboard"],
    );
    assert_eq!(code(&out), 2, "write mode (no -o) is unsupported");
    let out = run_shim(shims::render_xclip, Spool::Text, &["-i", "somefile"]);
    assert_eq!(code(&out), 2);
    assert!(!out.stderr.is_empty());
}

#[test]
fn wl_paste_list_rows() {
    let out = run_shim(shims::render_wl_paste, Spool::Image, &["-l"]);
    assert_eq!(code(&out), 0);
    assert!(String::from_utf8(out.stdout).unwrap().contains("image/png"));

    let out = run_shim(shims::render_wl_paste, Spool::Text, &["-l"]);
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("text/plain;charset=utf-8"));

    let out = run_shim(shims::render_wl_paste, Spool::Empty, &["-l"]);
    assert_eq!(code(&out), 1);
}

#[test]
fn wl_paste_read_rows() {
    let out = run_shim(
        shims::render_wl_paste,
        Spool::Image,
        &["--type", "image/png"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, PNG_BYTES);

    let out = run_shim(
        shims::render_wl_paste,
        Spool::Text,
        &["--type", "image/png"],
    );
    assert_eq!(code(&out), 1);

    let out = run_shim(
        shims::render_wl_paste,
        Spool::Image,
        &["--type", "image/bmp"],
    );
    assert_eq!(code(&out), 1);

    let out = run_shim(shims::render_wl_paste, Spool::Text, &[]);
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes());

    let out = run_shim(shims::render_wl_paste, Spool::Empty, &[]);
    assert_eq!(code(&out), 1);

    let out = run_shim(shims::render_wl_paste, Spool::Text, &["--watch"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn rendered_shims_carry_marker_and_version() {
    let s = shims::render_xclip("'/tmp/x'");
    assert!(s.contains(shims::MARKER_PREFIX));
    assert!(s.contains(env!("CARGO_PKG_VERSION")));
    assert!(!s.contains("__SPOOL__") && !s.contains("__VERSION__"));
}

#[test]
fn claude_code_detection_pipeline_matches_image() {
    let dir = tempdir();
    fs::write(dir.join("clip.png"), PNG_BYTES).unwrap();
    let shim = dir.join("xclip");
    fs::write(&shim, shims::render_xclip(&format!("'{}'", dir.display()))).unwrap();
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            r#"sh {} -selection clipboard -t TARGETS -o 2>/dev/null | grep -E "image/(png|jpeg|jpg|gif|webp|bmp)""#,
            shim.display()
        ))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code().unwrap(),
        0,
        "the exact Claude Code detection pipeline must match"
    );
}
