//! The image pipeline (SEC-11): every uploaded image, from the editor or a
//! quiz zip, is decoded from its bytes (not its name or Content-Type),
//! turned upright, shrunk to at most 1920 px, and re-encoded as WebP. Only
//! pixels survive, so EXIF data such as GPS location is gone, and a file
//! that is not really an image is refused. Files get random names.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use rand::{rngs::OsRng, TryRngCore};

/// Largest upload accepted (FR-QZ-01).
pub const MAX_INPUT_BYTES: usize = 5 * 1024 * 1024;
/// Longest side after resizing.
pub const MAX_SIDE: u32 = 1920;
/// Refuse absurd dimensions before allocating (decompression bombs).
const MAX_DECODE_SIDE: u32 = 12_000;
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;
const WEBP_QUALITY: f32 = 80.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaError {
    /// Not a JPEG, PNG or WebP, or damaged.
    NotAnImage,
    TooLarge,
    /// Dimensions beyond what we decode.
    TooManyPixels,
}

impl MediaError {
    pub fn code(self) -> &'static str {
        match self {
            MediaError::NotAnImage => "NOT_AN_IMAGE",
            MediaError::TooLarge => "FILE_TOO_LARGE",
            MediaError::TooManyPixels => "IMAGE_TOO_BIG",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Processed {
    pub webp: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decodes, orients, resizes and re-encodes. CPU heavy: call it from
/// `spawn_blocking`.
pub fn process(bytes: &[u8]) -> Result<Processed, MediaError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(MediaError::TooLarge);
    }
    let format = image::guess_format(bytes).map_err(|_| MediaError::NotAnImage)?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    ) {
        return Err(MediaError::NotAnImage);
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);

    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let orientation = decoder.orientation().map_err(decode_error)?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    img.apply_orientation(orientation);

    if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::CatmullRom);
    }
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let webp = webp::Encoder::from_rgba(rgba.as_raw(), width, height)
        .encode(WEBP_QUALITY)
        .to_vec();
    Ok(Processed {
        webp,
        width,
        height,
    })
}

fn decode_error(e: image::ImageError) -> MediaError {
    match e {
        image::ImageError::Limits(_) => MediaError::TooManyPixels,
        _ => MediaError::NotAnImage,
    }
}

/// A fresh random file name, e.g. `3f9c…e1.webp`.
pub fn new_storage_key() -> String {
    let mut b = [0u8; 16];
    OsRng.try_fill_bytes(&mut b).expect("OS random generator");
    format!("{}.webp", hex::encode(b))
}

/// Only names we generated are ever turned into paths.
pub fn is_storage_key(key: &str) -> bool {
    key.len() == 37 && key.ends_with(".webp") && key[..32].bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn path_for(dir: &Path, key: &str) -> PathBuf {
    debug_assert!(is_storage_key(key));
    dir.join(key)
}

/// Writes the file under a temporary name and renames it into place, so a
/// half-written image is never served.
pub async fn store(dir: &Path, key: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dir.join(format!(".tmp-{key}"));
    tokio::fs::write(&tmp, bytes).await?;
    if let Err(e) = tokio::fs::rename(&tmp, path_for(dir, key)).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    Ok(())
}

pub async fn remove(dir: &Path, key: &str) {
    if is_storage_key(key) {
        let _ = tokio::fs::remove_file(path_for(dir, key)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn encode(img: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn sample(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }))
    }

    #[test]
    fn png_and_jpeg_become_webp() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg] {
            let out = process(&encode(&sample(300, 200), format)).unwrap();
            assert_eq!((out.width, out.height), (300, 200));
            assert_eq!(&out.webp[..4], b"RIFF");
            assert_eq!(&out.webp[8..12], b"WEBP");
        }
    }

    #[test]
    fn large_images_are_shrunk_keeping_the_ratio() {
        let out = process(&encode(&sample(3840, 1000), ImageFormat::Png)).unwrap();
        assert_eq!((out.width, out.height), (1920, 500));
    }

    #[test]
    fn non_images_are_refused_by_content() {
        assert_eq!(
            process(b"<svg onload=alert(1)>").unwrap_err(),
            MediaError::NotAnImage
        );
        assert_eq!(process(b"GIF89a....").unwrap_err(), MediaError::NotAnImage);
        let mut png = encode(&sample(50, 50), ImageFormat::Png);
        png.truncate(60); // damaged
        assert_eq!(process(&png).unwrap_err(), MediaError::NotAnImage);
        assert_eq!(
            process(&vec![0u8; MAX_INPUT_BYTES + 1]).unwrap_err(),
            MediaError::TooLarge
        );
    }

    #[test]
    fn huge_dimensions_are_refused_before_decoding() {
        // A valid PNG header claiming 20000 x 20000 pixels.
        let mut png = encode(&sample(1, 1), ImageFormat::Png);
        png[16..20].copy_from_slice(&20_000u32.to_be_bytes());
        png[20..24].copy_from_slice(&20_000u32.to_be_bytes());
        assert!(process(&png).is_err());
    }

    #[test]
    fn storage_keys_are_random_and_checked() {
        let a = new_storage_key();
        assert!(is_storage_key(&a));
        assert_ne!(a, new_storage_key());
        assert!(!is_storage_key("../../etc/passwd.webp"));
        assert!(!is_storage_key("x.webp"));
    }
}
