// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::Cursor;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use livi_core_proto::icon::SIZES;

fn png(img: &DynamicImage, size: u32) -> Result<String, String> {
    let mut out = Vec::new();
    img.resize_to_fill(size, size, FilterType::Lanczos3)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(STANDARD.encode(out))
}

pub fn icons(img: &DynamicImage) -> Result<[String; 3], String> {
    let largest = SIZES.iter().map(|(_, s)| *s).max().unwrap_or(256);
    let base = if img.width().min(img.height()) > largest * 2 {
        img.resize_to_fill(largest * 2, largest * 2, FilterType::Triangle)
    } else {
        img.clone()
    };
    Ok([
        png(&base, SIZES[0].1)?,
        png(&base, SIZES[1].1)?,
        png(&base, SIZES[2].1)?,
    ])
}

pub fn import(path: &Path) -> Result<[String; 3], String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    icons(&img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn decoded(b64: &str) -> DynamicImage {
        image::load_from_memory(&STANDARD.decode(b64).unwrap()).unwrap()
    }

    #[test]
    fn a_wide_picture_is_cropped_to_squares_of_every_size() {
        let mut img = RgbaImage::from_pixel(900, 300, Rgba([0, 0, 255, 255]));
        for y in 0..300 {
            for x in 300..600 {
                img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let pngs = icons(&DynamicImage::ImageRgba8(img)).unwrap();
        for (b64, (_, size)) in pngs.iter().zip(SIZES) {
            let out = decoded(b64).into_rgba8();
            assert_eq!(out.dimensions(), (size, size));
            assert_eq!(out.get_pixel(size / 2, size / 2), &Rgba([255, 0, 0, 255]));
        }
    }

    #[test]
    fn a_huge_picture_still_gives_the_three_sizes() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2000, 1500, Rgba([1, 2, 3, 255])));
        let pngs = icons(&img).unwrap();
        assert_eq!(decoded(&pngs[2]).into_rgba8().dimensions(), (256, 256));
    }
}
