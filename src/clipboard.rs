use anyhow::{Context, Result, bail};
use image::{ImageBuffer, Rgba};

pub enum Payload {
    Text(String),
    Png(Vec<u8>),
}

impl Payload {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Payload::Text(s) => s.as_bytes(),
            Payload::Png(b) => b,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Payload::Text(_) => "text",
            Payload::Png(_) => "image",
        }
    }

    pub fn keep_file(&self) -> &'static str {
        match self {
            Payload::Text(_) => "clip.txt",
            Payload::Png(_) => "clip.png",
        }
    }

    pub fn drop_file(&self) -> &'static str {
        match self {
            Payload::Text(_) => "clip.png",
            Payload::Png(_) => "clip.txt",
        }
    }

    pub fn kind_mime(&self) -> &'static str {
        match self {
            Payload::Text(_) => "text/plain",
            Payload::Png(_) => "image/png",
        }
    }
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba.to_vec())
        .context("clipboard image dimensions do not match data")?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    Ok(out)
}

const EMPTY: &str =
    "clipboard is empty or holds unsupported content (ssh-paste sends text and images)";

pub fn read() -> Result<Payload> {
    let mut cb = arboard::Clipboard::new().context("opening clipboard")?;
    if let Ok(img) = cb.get_image() {
        let png = encode_png(img.width as u32, img.height as u32, &img.bytes)?;
        return Ok(Payload::Png(png));
    }
    if let Ok(text) = cb.get_text()
        && !text.is_empty()
    {
        return Ok(Payload::Text(text));
    }
    bail!(EMPTY);
}

pub fn peek_kind() -> Result<&'static str> {
    let mut cb = arboard::Clipboard::new().context("opening clipboard")?;
    if cb.get_image().is_ok() {
        return Ok("image/png");
    }
    if let Ok(text) = cb.get_text()
        && !text.is_empty()
    {
        return Ok("text/plain");
    }
    bail!(EMPTY);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_accessors() {
        let t = Payload::Text("hi".into());
        assert_eq!(t.kind(), "text");
        assert_eq!(t.keep_file(), "clip.txt");
        assert_eq!(t.drop_file(), "clip.png");
        assert_eq!(t.bytes(), b"hi");

        let p = Payload::Png(vec![1, 2]);
        assert_eq!(p.kind(), "image");
        assert_eq!(p.keep_file(), "clip.png");
        assert_eq!(p.drop_file(), "clip.txt");
    }

    #[test]
    fn encodes_rgba_to_decodable_png() {
        let png = encode_png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (2, 1));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }
}
