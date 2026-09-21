use image::imageops::FilterType;
use image::GenericImageView;

use crate::errors::{MhError, MhResult};

const TARGET: u32 = 640;

/// Decode any supported image. Shared so every caller reports the same thing
/// when a file is not a readable image.
pub fn decode_image(bytes: &[u8]) -> MhResult<image::DynamicImage> {
    image::load_from_memory(bytes).map_err(|e| {
        MhError::Other(format!(
            "Could not read this image. Try a PNG, JPEG, or WebP. ({e})"
        ))
    })
}

/// Encode as JPEG. Flattening to RGB first is required: JPEG has no alpha
/// channel, so encoding an RGBA image directly fails.
pub fn encode_jpeg<W: std::io::Write + std::io::Seek>(
    img: &image::DynamicImage,
    sink: &mut W,
) -> MhResult<()> {
    img.to_rgb8()
        .write_to(sink, image::ImageFormat::Jpeg)
        .map_err(|e| MhError::Other(format!("JPEG encode failed: {e}")))
}

pub fn to_square_jpeg(bytes: &[u8]) -> MhResult<Vec<u8>> {
    let img = decode_image(bytes)?;
    let (w, h) = img.dimensions();
    let side = w.min(h);
    let x = (w - side) / 2;
    let y = (h - side) / 2;
    let square = img.crop_imm(x, y, side, side);
    let resized = if side != TARGET {
        square.resize_exact(TARGET, TARGET, FilterType::Lanczos3)
    } else {
        square
    };
    let mut out = Vec::new();
    encode_jpeg(&resized, &mut std::io::Cursor::new(&mut out))?;
    Ok(out)
}

pub fn normalize_cover_base64(bytes: &[u8]) -> MhResult<String> {
    use base64::Engine;
    let jpeg = to_square_jpeg(bytes)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg))
}
