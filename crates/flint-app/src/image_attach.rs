//! Validated image attachments for native and ACP prompts.

use std::io::{Cursor, Write as _};
use std::path::Path;

use base64::Engine as _;
use flint_agent::ImageAttachment;
use gpui_kit::{Image, ImageFormat};

pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_IMAGES: usize = 4;

/// Clipboard TIFF/BMP data is often uncompressed; convert it off the UI thread.
pub fn clipboard_file(image: Image) -> Result<tempfile::NamedTempFile, String> {
    let (bytes, extension) = match image.format {
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp => {
            (image.bytes, image.format.extension())
        }
        ImageFormat::Tiff | ImageFormat::Bmp => {
            if image.bytes.len() > 64 * 1024 * 1024 {
                return Err("Clipboard image is too large to prepare (64 MB maximum).".into());
            }
            let format = if image.format == ImageFormat::Tiff {
                image::ImageFormat::Tiff
            } else {
                image::ImageFormat::Bmp
            };
            let mut reader = image::ImageReader::with_format(Cursor::new(image.bytes), format);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(16_384);
            limits.max_image_height = Some(16_384);
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            let decoded = reader
                .decode()
                .map_err(|err| format!("Couldn't paste image: {err}"))?;
            let mut png = Cursor::new(Vec::new());
            decoded
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|err| format!("Couldn't prepare image: {err}"))?;
            (png.into_inner(), "png")
        }
        _ => return Err("Paste a PNG, JPEG, GIF, WebP, TIFF or BMP image.".into()),
    };
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err("Pasted images must be 5 MB or smaller.".into());
    }
    if mime_type(&bytes).is_none() {
        return Err("Clipboard does not contain a supported image.".into());
    }
    let mut file = tempfile::Builder::new()
        .prefix("pasted-image-")
        .suffix(&format!(".{extension}"))
        .tempfile()
        .map_err(|err| format!("Couldn't store pasted image: {err}"))?;
    file.write_all(&bytes)
        .map_err(|err| format!("Couldn't store pasted image: {err}"))?;
    Ok(file)
}

pub fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

pub fn load(path: &Path) -> Result<ImageAttachment, String> {
    let name = name(path);
    let metadata = std::fs::metadata(path).map_err(|err| format!("{name}: {err}"))?;
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(format!("{name}: images must be 5 MB or smaller"));
    }
    let bytes = std::fs::read(path).map_err(|err| format!("{name}: {err}"))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(format!("{name}: images must be 5 MB or smaller"));
    }
    let mime_type = mime_type(&bytes)
        .ok_or_else(|| format!("{name}: choose a PNG, JPEG, GIF or WebP image"))?;
    Ok(ImageAttachment {
        name,
        mime_type: mime_type.into(),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

fn mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_images_and_rejects_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("picture.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\nimage").unwrap();
        let image = load(&png).unwrap();
        assert_eq!(image.name, "picture.png");
        assert_eq!(image.mime_type, "image/png");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(image.data)
                .unwrap(),
            b"\x89PNG\r\n\x1a\nimage"
        );
        std::fs::write(&png, b"not an image").unwrap();
        assert!(load(&png).unwrap_err().contains("choose a PNG"));
    }

    #[test]
    fn rejects_oversized_images() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("huge.png");
        let file = std::fs::File::create(&png).unwrap();
        file.set_len(MAX_IMAGE_BYTES + 1).unwrap();
        assert!(load(&png).unwrap_err().contains("5 MB"));
    }

    #[test]
    fn clipboard_images_use_private_temporary_files_and_clean_up() {
        let bytes = b"\x89PNG\r\n\x1a\nimage";
        let file = clipboard_file(Image::from_bytes(ImageFormat::Png, bytes.to_vec())).unwrap();
        let path = file.path().to_path_buf();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(load(&path).unwrap().mime_type, "image/png");
        drop(file);
        assert!(!path.exists());
    }

    #[test]
    fn native_clipboard_tiff_and_bmp_are_converted_to_png() {
        for (format, clipboard_format) in [
            (image::ImageFormat::Tiff, ImageFormat::Tiff),
            (image::ImageFormat::Bmp, ImageFormat::Bmp),
        ] {
            let original = image::DynamicImage::new_rgb8(2, 3);
            let mut bytes = Cursor::new(Vec::new());
            original.write_to(&mut bytes, format).unwrap();
            let file =
                clipboard_file(Image::from_bytes(clipboard_format, bytes.into_inner())).unwrap();
            let decoded = image::open(file.path()).unwrap();
            assert_eq!(decoded.width(), 2);
            assert_eq!(decoded.height(), 3);
            assert_eq!(load(file.path()).unwrap().mime_type, "image/png");
        }
    }

    #[test]
    fn invalid_and_oversized_clipboard_images_report_errors() {
        assert!(
            clipboard_file(Image::from_bytes(
                ImageFormat::Png,
                b"not an image".to_vec()
            ))
            .unwrap_err()
            .contains("supported image")
        );
        assert!(
            clipboard_file(Image::from_bytes(ImageFormat::Tiff, b"invalid".to_vec()))
                .unwrap_err()
                .contains("Couldn't paste image")
        );
        assert!(
            clipboard_file(Image::from_bytes(
                ImageFormat::Png,
                vec![0; MAX_IMAGE_BYTES as usize + 1],
            ))
            .unwrap_err()
            .contains("5 MB")
        );
    }
}
