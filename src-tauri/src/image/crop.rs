//! Square crop for images that are about to be uploaded to the Developer Portal.
//!
//! Discord shows a large image as a square and centers non-square images by
//! itself; there is no setting to control that. The only thing that can be
//! controlled is the image Discord receives, so the app cuts the wanted square
//! out of the original and saves it as a 1024×1024 PNG, which is then uploaded
//! as the asset (Discord has no upload API; the account token is never used).

use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::MAX_IMAGE_BYTES;
use crate::error::{AppError, Result};

/// Discord recommends 1024×1024 for every Rich Presence image.
pub const EXPORT_SIZE: u32 = 1024;
const MIN_CROP: u32 = 16;

/// The square to keep, in pixels of the original image.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}

/// Cuts `rect` out of the image in `bytes` and scales it to `out`×`out`.
pub fn crop_square(bytes: &[u8], rect: CropRect, out: u32) -> Result<Vec<u8>> {
    if !(64..=2048).contains(&out) {
        return Err(AppError::Invalid("출력 크기가 올바르지 않습니다.".into()));
    }
    let decoded = image::load_from_memory(bytes).map_err(|e| {
        AppError::Image(format!(
            "이 이미지는 자를 수 없습니다 (PNG, JPEG, WebP, GIF만 가능): {e}"
        ))
    })?;
    let (width, height) = (decoded.width(), decoded.height());
    let right = rect.x.checked_add(rect.size);
    let bottom = rect.y.checked_add(rect.size);
    if rect.size < MIN_CROP
        || right.is_none_or(|r| r > width)
        || bottom.is_none_or(|b| b > height)
    {
        return Err(AppError::Invalid(format!(
            "자를 영역이 이미지({width}×{height})를 벗어났거나 너무 작습니다."
        )));
    }

    let cropped = decoded
        .crop_imm(rect.x, rect.y, rect.size, rect.size)
        .resize_exact(out, out, image::imageops::FilterType::Lanczos3);
    let mut png = Vec::new();
    cropped
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| AppError::Image(format!("PNG로 저장할 수 없습니다: {e}")))?;
    Ok(png)
}

/// A file name that is safe on every OS: lowercase letters, digits, `-` and `_`.
/// The Developer Portal lowercases asset keys, so the export is named like one.
pub fn safe_name(name: &str) -> String {
    let mut safe = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            safe.push(ch.to_ascii_lowercase());
        } else if !safe.ends_with('-') {
            safe.push('-');
        }
    }
    let safe: String = safe.trim_matches('-').chars().take(64).collect();
    if safe.is_empty() {
        "cover".to_string()
    } else {
        safe
    }
}

pub struct Exported {
    pub path: PathBuf,
    pub bytes: u64,
}

/// Reads `source`, crops it and writes `<dir>/<file_name>` (replacing an
/// earlier export of the same name) via a temporary file.
pub fn export_file(source: &Path, rect: CropRect, dir: &Path, file_name: &str) -> Result<Exported> {
    let meta = fs::metadata(source)
        .map_err(|e| AppError::Image(format!("원본 이미지를 열 수 없습니다: {e}")))?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(AppError::Image("원본 이미지가 너무 큽니다.".into()));
    }
    let png = crop_square(&fs::read(source)?, rect, EXPORT_SIZE)?;

    fs::create_dir_all(dir)?;
    let target = dir.join(file_name);
    let tmp = dir.join(format!("{file_name}.tmp"));
    let write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&png)?;
        file.sync_all()?;
        fs::rename(&tmp, &target)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        AppError::Storage(format!("{}: {e}", target.display()))
    })?;
    Ok(Exported {
        path: target,
        bytes: png.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 200×100: left half red, right half blue, with a green marker pixel.
    fn two_tone() -> Vec<u8> {
        let mut img = image::RgbaImage::from_pixel(200, 100, image::Rgba([255, 0, 0, 255]));
        for x in 100..200 {
            for y in 0..100 {
                img.put_pixel(x, y, image::Rgba([0, 0, 255, 255]));
            }
        }
        img.put_pixel(150, 50, image::Rgba([0, 255, 0, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn decode(png: &[u8]) -> image::RgbaImage {
        image::load_from_memory(png).unwrap().to_rgba8()
    }

    #[test]
    fn the_chosen_region_is_what_ends_up_in_the_output() {
        let right = crop_square(&two_tone(), CropRect { x: 100, y: 0, size: 100 }, 64).unwrap();
        let img = decode(&right);
        assert_eq!(img.dimensions(), (64, 64));
        assert_eq!(img.get_pixel(5, 5).0, [0, 0, 255, 255], "the right half is blue");

        let left = decode(&crop_square(&two_tone(), CropRect { x: 0, y: 0, size: 100 }, 64).unwrap());
        assert_eq!(left.get_pixel(5, 5).0, [255, 0, 0, 255], "the left half is red");
    }

    #[test]
    fn a_smaller_region_is_scaled_up_to_the_output_size() {
        // 20×20 around the green pixel at (150, 50).
        let png = crop_square(&two_tone(), CropRect { x: 140, y: 40, size: 20 }, 128).unwrap();
        let img = decode(&png);
        assert_eq!(img.dimensions(), (128, 128));
        // The marker is crop pixel (10, 10), which covers output 64..70.4 when
        // scaled 6.4x. Sample its middle, not the edge it shares with the blue.
        let marker = img.get_pixel(67, 67).0;
        assert!(marker[1] > 200 && marker[2] < 60, "green marker at the centre: {marker:?}");
        let beside = img.get_pixel(20, 20).0;
        assert!(beside[2] > 200 && beside[1] < 60, "blue elsewhere: {beside:?}");
    }

    #[test]
    fn regions_outside_the_image_or_too_small_are_rejected() {
        let bytes = two_tone();
        for rect in [
            CropRect { x: 150, y: 0, size: 100 },   // sticks out to the right
            CropRect { x: 0, y: 50, size: 100 },    // sticks out at the bottom
            CropRect { x: 0, y: 0, size: 101 },     // taller than the image
            CropRect { x: 0, y: 0, size: 8 },       // too small
            CropRect { x: u32::MAX, y: 0, size: 50 }, // overflow
        ] {
            assert!(crop_square(&bytes, rect, 64).is_err(), "{rect:?}");
        }
    }

    #[test]
    fn non_images_and_bad_output_sizes_are_rejected() {
        let rect = CropRect { x: 0, y: 0, size: 50 };
        assert!(crop_square(b"not an image", rect, 64).is_err());
        assert!(crop_square(&two_tone(), rect, 8).is_err());
        assert!(crop_square(&two_tone(), rect, 9000).is_err());
    }

    #[test]
    fn export_names_are_safe_and_keyed_like_portal_assets() {
        assert_eq!(safe_name("Elden Ring"), "elden-ring");
        assert_eq!(safe_name("  ocarina_of_time "), "ocarina_of_time");
        assert_eq!(safe_name("../../etc/passwd"), "etc-passwd");
        assert_eq!(safe_name("포켓몬"), "cover", "nothing usable falls back to a default");
        assert_eq!(safe_name(&"a".repeat(200)).len(), 64);
        assert!(!safe_name("a/b\\c:d").contains(['/', '\\', ':']));
    }

    #[test]
    fn export_writes_a_1024_png_and_replaces_an_older_export() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.png");
        fs::write(&source, two_tone()).unwrap();
        let out = dir.path().join("exports");

        let first = export_file(&source, CropRect { x: 0, y: 0, size: 100 }, &out, "cover.png").unwrap();
        let img = decode(&fs::read(&first.path).unwrap());
        assert_eq!(img.dimensions(), (EXPORT_SIZE, EXPORT_SIZE));
        assert_eq!(img.get_pixel(5, 5).0, [255, 0, 0, 255]);

        let second = export_file(&source, CropRect { x: 100, y: 0, size: 100 }, &out, "cover.png").unwrap();
        assert_eq!(first.path, second.path);
        assert_eq!(decode(&fs::read(&second.path).unwrap()).get_pixel(5, 5).0, [0, 0, 255, 255]);
        let leftovers = fs::read_dir(&out).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary file is left behind");
    }

    #[test]
    fn export_reports_a_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let err = export_file(
            &dir.path().join("missing.png"),
            CropRect { x: 0, y: 0, size: 50 },
            dir.path(),
            "x.png",
        );
        assert!(err.is_err());
    }
}
