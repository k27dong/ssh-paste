use std::fs;
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use ssh_paste::clipboard::Payload;
use ssh_paste::{serve, shims};

const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nfakebody";
const TEXT: &str = "hello from mac";

enum Spool {
    Image,
    Text,
    Empty,
}

fn run_shim(render: fn(&str, u16) -> String, spool: Spool, port: u16, args: &[&str]) -> Output {
    let dir = tempdir();
    match spool {
        Spool::Image => fs::write(dir.join("clip.png"), PNG_BYTES).unwrap(),
        Spool::Text => fs::write(dir.join("clip.txt"), TEXT).unwrap(),
        Spool::Empty => {}
    }
    let script = render(&format!("'{}'", dir.display()), port);
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

fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

fn start_tunnel(source: impl Fn() -> anyhow::Result<Payload> + Send + 'static) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = serve::handle_one(stream, &source);
        }
    });
    port
}

#[test]
fn xclip_targets_row() {
    let port = closed_port();
    let out = run_shim(
        shims::render_xclip,
        Spool::Image,
        port,
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
        port,
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
    );
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("text/plain") && !stdout.contains("image/png"));

    let out = run_shim(
        shims::render_xclip,
        Spool::Empty,
        port,
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
    );
    assert_eq!(code(&out), 1);
}

#[test]
fn xclip_image_read_rows() {
    let port = closed_port();
    let out = run_shim(
        shims::render_xclip,
        Spool::Image,
        port,
        &["-selection", "clipboard", "-t", "image/png", "-o"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, PNG_BYTES);

    for spool in [Spool::Text, Spool::Empty] {
        let out = run_shim(
            shims::render_xclip,
            spool,
            port,
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        );
        assert_eq!(code(&out), 1);
        assert!(out.stdout.is_empty());
    }

    let out = run_shim(
        shims::render_xclip,
        Spool::Image,
        port,
        &["-selection", "clipboard", "-t", "image/bmp", "-o"],
    );
    assert_eq!(code(&out), 1, "only png is stored; bmp must be unavailable");
}

#[test]
fn xclip_text_read_rows() {
    let port = closed_port();
    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        port,
        &["-selection", "clipboard", "-t", "text/plain", "-o"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes(), "no added trailing newline");

    let out = run_shim(shims::render_xclip, Spool::Text, port, &["-o"]);
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes());

    let out = run_shim(
        shims::render_xclip,
        Spool::Image,
        port,
        &["-selection", "clipboard", "-t", "text/plain", "-o"],
    );
    assert_eq!(code(&out), 1);
}

#[test]
fn xclip_rejects_unsupported_loudly() {
    let port = closed_port();
    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        port,
        &["-selection", "clipboard"],
    );
    assert_eq!(code(&out), 2, "write mode (no -o) is unsupported");
    let out = run_shim(shims::render_xclip, Spool::Text, port, &["-i", "somefile"]);
    assert_eq!(code(&out), 2);
    assert!(!out.stderr.is_empty());
}

#[test]
fn wl_paste_list_rows() {
    let port = closed_port();
    let out = run_shim(shims::render_wl_paste, Spool::Image, port, &["-l"]);
    assert_eq!(code(&out), 0);
    assert!(String::from_utf8(out.stdout).unwrap().contains("image/png"));

    let out = run_shim(shims::render_wl_paste, Spool::Text, port, &["-l"]);
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("text/plain;charset=utf-8"));

    let out = run_shim(shims::render_wl_paste, Spool::Empty, port, &["-l"]);
    assert_eq!(code(&out), 1);
}

#[test]
fn wl_paste_read_rows() {
    let port = closed_port();
    let out = run_shim(
        shims::render_wl_paste,
        Spool::Image,
        port,
        &["--type", "image/png"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, PNG_BYTES);

    let out = run_shim(
        shims::render_wl_paste,
        Spool::Text,
        port,
        &["--type", "image/png"],
    );
    assert_eq!(code(&out), 1);

    let out = run_shim(
        shims::render_wl_paste,
        Spool::Image,
        port,
        &["--type", "image/bmp"],
    );
    assert_eq!(code(&out), 1);

    let out = run_shim(shims::render_wl_paste, Spool::Text, port, &[]);
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes());

    let out = run_shim(shims::render_wl_paste, Spool::Empty, port, &[]);
    assert_eq!(code(&out), 1);

    let out = run_shim(shims::render_wl_paste, Spool::Text, port, &["--watch"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn rendered_shims_carry_marker_and_version() {
    let s = shims::render_xclip("'/tmp/x'", 7717);
    assert!(s.contains(shims::MARKER_PREFIX));
    assert!(s.contains(env!("CARGO_PKG_VERSION")));
    assert!(!s.contains("__SPOOL__") && !s.contains("__VERSION__") && !s.contains("__PORT__"));
}

#[test]
fn claude_code_detection_pipeline_matches_image() {
    let dir = tempdir();
    fs::write(dir.join("clip.png"), PNG_BYTES).unwrap();
    let shim = dir.join("xclip");
    fs::write(
        &shim,
        shims::render_xclip(&format!("'{}'", dir.display()), closed_port()),
    )
    .unwrap();
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

#[test]
fn tunnel_serves_live_image_over_spool() {
    let port = start_tunnel(|| Ok(Payload::Png(PNG_BYTES.to_vec())));

    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        port,
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
    );
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("image/png") && !stdout.contains("text/plain"),
        "live tunnel kind must win over the stale spool text file: {stdout}"
    );

    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        port,
        &["-selection", "clipboard", "-t", "image/png", "-o"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, PNG_BYTES);

    let out = run_shim(
        shims::render_wl_paste,
        Spool::Text,
        port,
        &["--type", "image/png"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, PNG_BYTES);
}

#[test]
fn tunnel_serves_live_text() {
    let port = start_tunnel(|| Ok(Payload::Text("live text".into())));

    let out = run_shim(
        shims::render_xclip,
        Spool::Empty,
        port,
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
    );
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("text/plain"));

    let out = run_shim(
        shims::render_xclip,
        Spool::Empty,
        port,
        &["-selection", "clipboard", "-t", "text/plain", "-o"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"live text");

    let out = run_shim(shims::render_wl_paste, Spool::Empty, port, &[]);
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"live text");
}

#[test]
fn dead_tunnel_falls_back_to_spool() {
    let port = closed_port();

    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        port,
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
    );
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("text/plain") && !stdout.contains("image/png"));

    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        port,
        &["-selection", "clipboard", "-t", "text/plain", "-o"],
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes());

    let out = run_shim(shims::render_xclip, Spool::Text, port, &["-o"]);
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes());

    let out = run_shim(
        shims::render_xclip,
        Spool::Text,
        port,
        &["-selection", "clipboard", "-t", "image/png", "-o"],
    );
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());

    let out = run_shim(shims::render_wl_paste, Spool::Text, port, &["-l"]);
    assert_eq!(code(&out), 0);
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("text/plain;charset=utf-8")
    );

    let out = run_shim(shims::render_wl_paste, Spool::Text, port, &[]);
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, TEXT.as_bytes());

    let out = run_shim(
        shims::render_wl_paste,
        Spool::Text,
        port,
        &["--type", "image/png"],
    );
    assert_eq!(code(&out), 1);
}

#[test]
fn tunnel_kind_change_between_calls_reads_as_empty() {
    let calls = AtomicUsize::new(0);
    let port = start_tunnel(move || {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(Payload::Png(PNG_BYTES.to_vec()))
        } else {
            Ok(Payload::Text(TEXT.into()))
        }
    });

    let out = run_shim(
        shims::render_xclip,
        Spool::Empty,
        port,
        &["-selection", "clipboard", "-t", "image/png", "-o"],
    );
    assert_eq!(
        code(&out),
        22,
        "curl -f surfaces the 404 from a kind/data mismatch as CURLE_HTTP_RETURNED_ERROR"
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn wl_paste_render_carries_marker_and_detection_pipeline_matches() {
    let s = shims::render_wl_paste("'/tmp/x'", 7717);
    assert!(s.contains(shims::MARKER_PREFIX));
    assert!(s.contains(env!("CARGO_PKG_VERSION")));
    assert!(!s.contains("__SPOOL__") && !s.contains("__VERSION__") && !s.contains("__PORT__"));

    let port = start_tunnel(|| Ok(Payload::Png(PNG_BYTES.to_vec())));
    let dir = tempdir();
    let shim = dir.join("wl-paste");
    fs::write(
        &shim,
        shims::render_wl_paste(&format!("'{}'", dir.display()), port),
    )
    .unwrap();
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            r#"sh {} -l 2>/dev/null | grep -E "image/(png|jpeg|jpg|gif|webp|bmp)""#,
            shim.display()
        ))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code().unwrap(),
        0,
        "the exact Claude Code detection pipeline must match for wl-paste too"
    );
}
