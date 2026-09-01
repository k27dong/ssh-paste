use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use anyhow::Result;

use crate::clipboard::Payload;

pub fn serve(
    listener: TcpListener,
    kind_source: impl Fn() -> Result<&'static str>,
    data_source: impl Fn() -> Result<Payload>,
) -> Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                if let Err(err) = handle_one(s, &kind_source, &data_source) {
                    eprintln!("ssh-paste serve: {err:#}");
                }
            }
            Err(err) => eprintln!("ssh-paste serve: accept failed: {err}"),
        }
    }
    Ok(())
}

pub fn handle_one(
    mut stream: TcpStream,
    kind_source: &impl Fn() -> Result<&'static str>,
    data_source: &impl Fn() -> Result<Payload>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let path = match (parts.next(), parts.next()) {
        (Some("GET"), Some(p)) => p.to_string(),
        _ => return respond(&mut stream, 400, "text/plain", b"bad request"),
    };
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header == "\r\n" || header == "\n" {
            break;
        }
    }
    match path.as_str() {
        "/kind" => match kind_source() {
            Ok(kind) => respond(&mut stream, 200, "text/plain", kind.as_bytes()),
            Err(_) => unavailable(&mut stream),
        },
        "/data/image" => match data_source() {
            Ok(Payload::Png(b)) => respond(&mut stream, 200, "image/png", &b),
            _ => unavailable(&mut stream),
        },
        "/data/text" => match data_source() {
            Ok(Payload::Text(t)) => {
                respond(&mut stream, 200, "text/plain; charset=utf-8", t.as_bytes())
            }
            _ => unavailable(&mut stream),
        },
        _ => unavailable(&mut stream),
    }
}

fn unavailable(stream: &mut TcpStream) -> Result<()> {
    respond(stream, 404, "text/plain", b"clipboard unavailable")
}

fn respond(stream: &mut TcpStream, status: u16, ctype: &str, body: &[u8]) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn roundtrip(
        path: &'static str,
        kind_source: impl Fn() -> anyhow::Result<&'static str>,
        data_source: impl Fn() -> anyhow::Result<Payload>,
    ) -> (String, Vec<u8>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).unwrap();
            buf
        });
        let (stream, _) = listener.accept().unwrap();
        handle_one(stream, &kind_source, &data_source).unwrap();
        let raw = client.join().unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (
            String::from_utf8(raw[..split].to_vec()).unwrap(),
            raw[split + 4..].to_vec(),
        )
    }

    #[test]
    fn kind_reports_current_clipboard() {
        let (head, body) = roundtrip("/kind", || Ok("image/png"), || Ok(Payload::Png(vec![1])));
        assert!(head.starts_with("HTTP/1.1 200"));
        assert_eq!(body, b"image/png");
        let (head, body) = roundtrip(
            "/kind",
            || Ok("text/plain"),
            || Ok(Payload::Text("x".into())),
        );
        assert!(head.starts_with("HTTP/1.1 200"));
        assert_eq!(body, b"text/plain");
    }

    #[test]
    fn data_serves_matching_kind_only() {
        let png = vec![137, 80, 78, 71, 9, 9];
        let p = png.clone();
        let (head, body) = roundtrip(
            "/data/image",
            || Ok("image/png"),
            move || Ok(Payload::Png(p.clone())),
        );
        assert!(head.starts_with("HTTP/1.1 200"));
        assert!(head.contains("Content-Type: image/png"));
        assert!(head.contains(&format!("Content-Length: {}", png.len())));
        assert_eq!(body, png);

        let (head, _) = roundtrip(
            "/data/image",
            || Ok("text/plain"),
            || Ok(Payload::Text("now text".into())),
        );
        assert!(
            head.starts_with("HTTP/1.1 404"),
            "kind changed between calls must 404: {head}"
        );

        let (head, body) = roundtrip(
            "/data/text",
            || Ok("text/plain"),
            || Ok(Payload::Text("hé".into())),
        );
        assert!(head.starts_with("HTTP/1.1 200"));
        assert!(head.contains("Content-Type: text/plain; charset=utf-8"));
        assert_eq!(body, "hé".as_bytes());
    }

    #[test]
    fn kind_and_data_answer_from_their_own_source() {
        let (head, body) = roundtrip(
            "/kind",
            || Ok("image/png"),
            || Ok(Payload::Text("stale text".into())),
        );
        assert!(head.starts_with("HTTP/1.1 200"));
        assert_eq!(body, b"image/png");

        let (head, _) = roundtrip(
            "/data/image",
            || Ok("image/png"),
            || Ok(Payload::Text("stale text".into())),
        );
        assert!(
            head.starts_with("HTTP/1.1 404"),
            "a kind/data disagreement must stay a clean 404: {head}"
        );
    }

    #[test]
    fn unavailable_clipboard_and_bad_paths_fail_cleanly() {
        let (head, _) = roundtrip(
            "/kind",
            || anyhow::bail!("no clipboard"),
            || anyhow::bail!("no clipboard"),
        );
        assert!(head.starts_with("HTTP/1.1 404"));
        let (head, _) = roundtrip(
            "/nope",
            || Ok("text/plain"),
            || Ok(Payload::Text("x".into())),
        );
        assert!(head.starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn malformed_request_gets_400() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            s.write_all(b"garbage\r\n\r\n").unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).unwrap();
            buf
        });
        let (stream, _) = listener.accept().unwrap();
        handle_one(stream, &|| Ok("text/plain"), &|| {
            Ok(Payload::Text("x".into()))
        })
        .unwrap();
        assert!(client.join().unwrap().starts_with(b"HTTP/1.1 400"));
    }
}
