//! Decoding (design §16 steps 1–2). IO helpers for the app crate and CLI; generators
//! never call these.
//!
//! - Formats: PNG, JPEG, TIFF, WebP, BMP (the `image` crate features enabled in the
//!   workspace). The format is sniffed from the bytes, not the file name.
//! - The EXIF orientation is applied, so row 0 is the top of the picture as seen.
//! - Colour: sRGB → linear per channel, then `Y = 0.2126 R + 0.7152 G + 0.0722 B`.
//!   8-bit sources go through a 256-entry LUT, deeper sources through 16 bits.
//! - Alpha is composited over white in linear light (a transparent background reads as
//!   a fully lit facet rather than whatever colour the encoder left under it).

use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

use image::{DynamicImage, ImageDecoder};

use super::{LinearLuma, srgb_to_linear, srgb_to_linear_f64};
use crate::error::{Error, Result};

const KR: f32 = 0.2126;
const KG: f32 = 0.7152;
const KB: f32 = 0.0722;

fn lut8() -> &'static [f32; 256] {
    static L: OnceLock<[f32; 256]> = OnceLock::new();
    L.get_or_init(|| {
        let mut l = [0f32; 256];
        for (k, v) in l.iter_mut().enumerate() {
            *v = srgb_to_linear(k as f32 / 255.0);
        }
        l
    })
}

fn lut16() -> &'static [f32] {
    static L: OnceLock<Vec<f32>> = OnceLock::new();
    L.get_or_init(|| (0..65536u32).map(|k| srgb_to_linear_f64(k as f64 / 65535.0) as f32).collect())
}

/// Decodes an encoded image held in memory and applies its EXIF orientation.
pub fn decode_dynamic(bytes: &[u8]) -> Result<DynamicImage> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Error::Io(format!("image format: {e}")))?;
    if reader.format().is_none() {
        return Err(Error::Io("unrecognised image format".into()));
    }
    let mut dec = reader.into_decoder().map_err(|e| Error::Io(format!("decode: {e}")))?;
    let orient = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(|e| Error::Io(format!("decode: {e}")))?;
    img.apply_orientation(orient);
    Ok(img)
}

/// Decodes bytes (PNG/JPEG/TIFF/WebP/BMP), applies EXIF orientation and linearises.
pub fn decode_bytes(bytes: &[u8]) -> Result<LinearLuma> {
    let img = decode_dynamic(bytes)?;
    if img.width() == 0 || img.height() == 0 {
        return Err(Error::Io("image has no pixels".into()));
    }
    Ok(LinearLuma::from_dynamic(&img))
}

impl LinearLuma {
    /// sRGB → linear per channel, then `Y = 0.2126 R + 0.7152 G + 0.0722 B`; alpha is
    /// composited over white in linear light.
    pub fn from_dynamic(img: &DynamicImage) -> Self {
        let (w, h) = (img.width(), img.height());
        let deep = img.color().bytes_per_pixel() / img.color().channel_count().max(1) > 1;
        let px: Vec<f32> = match img {
            DynamicImage::ImageLuma8(g) => {
                let l = lut8();
                g.as_raw().iter().map(|&c| l[c as usize]).collect()
            }
            DynamicImage::ImageRgb8(rgb) => {
                let l = lut8();
                rgb.as_raw()
                    .chunks_exact(3)
                    .map(|c| KR * l[c[0] as usize] + KG * l[c[1] as usize] + KB * l[c[2] as usize])
                    .collect()
            }
            _ if !deep => {
                let l = lut8();
                img.to_rgba8()
                    .as_raw()
                    .chunks_exact(4)
                    .map(|c| {
                        let y = KR * l[c[0] as usize] + KG * l[c[1] as usize] + KB * l[c[2] as usize];
                        let a = c[3] as f32 / 255.0;
                        a * y + (1.0 - a)
                    })
                    .collect()
            }
            _ => {
                let l = lut16();
                img.to_rgba16()
                    .as_raw()
                    .chunks_exact(4)
                    .map(|c| {
                        let y = KR * l[c[0] as usize] + KG * l[c[1] as usize] + KB * l[c[2] as usize];
                        let a = c[3] as f32 / 65535.0;
                        a * y + (1.0 - a)
                    })
                    .collect()
            }
        };
        LinearLuma { w, h, px }
    }

    /// Reads and decodes a file (format sniffed from its bytes), applies its EXIF
    /// orientation and linearises it. IO helper for the CLI and app.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        decode_bytes(&bytes).map_err(|e| match e {
            Error::Io(m) => Error::Io(format!("{}: {m}", path.display())),
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(img: &DynamicImage) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn grey_png_round_trip_is_linearised() {
        let img = DynamicImage::ImageLuma8(image::GrayImage::from_raw(2, 1, vec![0, 255]).unwrap());
        let l = decode_bytes(&png_bytes(&img)).unwrap();
        assert_eq!((l.w, l.h), (2, 1));
        assert_eq!(l.px, vec![0.0, 1.0]);
    }

    #[test]
    fn mid_grey_is_linear_not_code_value() {
        let img = DynamicImage::ImageLuma8(image::GrayImage::from_raw(1, 1, vec![128]).unwrap());
        let l = decode_bytes(&png_bytes(&img)).unwrap();
        assert!((l.px[0] - 0.2158605).abs() < 1e-5, "{}", l.px[0]);
    }

    #[test]
    fn transparent_pixel_is_white() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_raw(2, 1, vec![0, 0, 0, 0, 0, 0, 0, 255]).unwrap());
        let l = decode_bytes(&png_bytes(&img)).unwrap();
        assert_eq!(l.px, vec![1.0, 0.0]);
    }

    #[test]
    fn sixteen_bit_uses_full_precision() {
        let img = DynamicImage::ImageLuma16(image::ImageBuffer::from_raw(2, 1, vec![0u16, 300]).unwrap());
        let l = decode_bytes(&png_bytes(&img)).unwrap();
        let want = srgb_to_linear_f64(300.0 / 65535.0) as f32;
        assert!((l.px[1] - want).abs() < 1e-9 && l.px[1] > 0.0);
    }

    #[test]
    fn garbage_is_an_io_error() {
        assert!(matches!(decode_bytes(b"not an image"), Err(Error::Io(_))));
    }
}
