//! Generated fixtures only; execute on the designated macOS test host.
use crate::{
    AppState, imaging,
    safety_tests::{register, state},
};
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::{fs, path::Path, process::Command};

fn preview(s: &AppState, path: &Path, size: u32) -> Vec<u8> {
    let (folder, id) = register(s, path);
    s.allowed
        .lock()
        .unwrap()
        .insert(folder, path.parent().unwrap().canonicalize().unwrap());
    imaging::render(s, &id, size).unwrap()
}
fn sips(args: &[&str], input: &Path, output: &Path) {
    let result = Command::new("/usr/bin/sips")
        .args(args)
        .arg(input)
        .arg("--out")
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn sdr_formats_are_bounded_and_keep_originals_and_color_profile() {
    let t = tempfile::tempdir().unwrap();
    let s = state(&t.path().join("runtime"));
    let pattern = image::RgbImage::from_fn(900, 600, |x, y| {
        image::Rgb([(x / 4) as u8, (y / 3) as u8, 70])
    });
    for ext in ["jpg", "png", "webp"] {
        let p = t.path().join(format!("中文 空格.{ext}"));
        pattern.save(&p).unwrap();
        let before = fs::read(&p).unwrap();
        for size in [480, 1600, 8192] {
            let data = preview(&s, &p, size);
            let mut decoder = ImageReader::new(std::io::Cursor::new(&data))
                .with_guessed_format()
                .unwrap()
                .into_decoder()
                .unwrap();
            let has_icc = decoder.icc_profile().unwrap().is_some();
            let exif_srgb = exif::Reader::new()
                .read_from_container(&mut std::io::Cursor::new(&data))
                .ok()
                .and_then(|e| {
                    e.get_field(exif::Tag::ColorSpace, exif::In::PRIMARY)
                        .and_then(|f| f.value.get_uint(0))
                })
                == Some(1);
            assert!(
                has_icc || exif_srgb,
                "preview must describe its sRGB pixels"
            );
            let out = image::load_from_memory(&data).unwrap();
            assert_eq!(
                (out.width(), out.height()),
                if size == 480 { (480, 320) } else { (900, 600) }
            );
        }
        assert_eq!(before, fs::read(p).unwrap());
    }
}

#[test]
fn all_eight_orientations_match_pixels_not_just_dimensions() {
    let t = tempfile::tempdir().unwrap();
    let s = state(&t.path().join("runtime"));
    let pixels = image::RgbImage::from_fn(80, 40, |x, y| {
        image::Rgb(match (x < 40, y < 20) {
            (true, true) => [220, 30, 30],
            (false, true) => [30, 220, 30],
            (true, false) => [30, 30, 220],
            (false, false) => [220, 220, 30],
        })
    });
    let mut base = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut base, 95)
        .encode_image(&pixels)
        .unwrap();
    for orientation in 1..=8u8 {
        let exif = [
            69,
            120,
            105,
            102,
            0,
            0,
            73,
            73,
            42,
            0,
            8,
            0,
            0,
            0,
            1,
            0,
            18,
            1,
            3,
            0,
            1,
            0,
            0,
            0,
            orientation,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        let mut bytes = base[..2].to_vec();
        bytes.extend_from_slice(&[255, 225]);
        bytes.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        bytes.extend_from_slice(&exif);
        bytes.extend_from_slice(&base[2..]);
        let path = t.path().join(format!("orientation-{orientation}.jpg"));
        fs::write(&path, &bytes).unwrap();
        let mut expected = DynamicImage::ImageRgb8(pixels.clone());
        expected.apply_orientation(image::metadata::Orientation::from_exif(orientation).unwrap());
        let expected = expected.to_rgb8();
        let actual = image::load_from_memory(&preview(&s, &path, 480))
            .unwrap()
            .to_rgb8();
        assert_eq!(actual.dimensions(), expected.dimensions());
        for x in [actual.width() / 4, actual.width() * 3 / 4] {
            for y in [actual.height() / 4, actual.height() * 3 / 4] {
                for (a, b) in actual
                    .get_pixel(x, y)
                    .0
                    .iter()
                    .zip(expected.get_pixel(x, y).0)
                {
                    assert!(
                        (i16::from(*a) - i16::from(b)).abs() < 15,
                        "orientation {orientation}: pixel mismatch"
                    );
                }
            }
        }
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn display_p3_preview_matches_independent_colorsync_conversion() {
    let t = tempfile::tempdir().unwrap();
    let s = state(&t.path().join("runtime"));
    let plain = t.path().join("plain.png");
    image::RgbImage::from_pixel(80, 40, image::Rgb([180, 100, 70]))
        .save(&plain)
        .unwrap();
    let p3 = t.path().join("p3.png");
    sips(
        &["-e", "/System/Library/ColorSync/Profiles/Display P3.icc"],
        &plain,
        &p3,
    );
    let before = fs::read(&p3).unwrap();
    let reference = t.path().join("reference.png");
    sips(
        &["-m", "/System/Library/ColorSync/Profiles/sRGB Profile.icc"],
        &p3,
        &reference,
    );
    let expected = image::open(reference)
        .unwrap()
        .to_rgb8()
        .get_pixel(40, 20)
        .0;
    let actual = image::load_from_memory(&preview(&s, &p3, 480))
        .unwrap()
        .to_rgb8()
        .get_pixel(40, 20)
        .0;
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (i16::from(*a) - i16::from(b)).abs() <= 5),
        "{actual:?} vs {expected:?}"
    );
    assert_eq!(fs::read(p3).unwrap(), before);
}

#[test]
fn high_depth_and_corrupt_inputs_fail_without_poisoning_other_previews() {
    let t = tempfile::tempdir().unwrap();
    let s = state(&t.path().join("runtime"));
    let high = t.path().join("high.png");
    image::ImageBuffer::from_pixel(40, 30, image::Rgb([50000u16, 20000, 10000]))
        .save(&high)
        .unwrap();
    let (folder, id) = register(&s, &high);
    s.allowed
        .lock()
        .unwrap()
        .insert(folder, t.path().canonicalize().unwrap());
    assert!(
        imaging::render(&s, &id, 480)
            .unwrap_err()
            .contains("高位深")
    );
    let bad = t.path().join("broken.heic");
    fs::write(&bad, b"not a photo").unwrap();
    let (_, id) = register(&s, &bad);
    assert!(imaging::render(&s, &id, 480).is_err());
    let good = t.path().join("good.jpg");
    image::RgbImage::new(40, 30).save(&good).unwrap();
    assert!(!preview(&s, &good, 480).is_empty());
}

#[test]
fn legacy_unmanaged_cache_is_not_reused() {
    let t = tempfile::tempdir().unwrap();
    let s = state(&t.path().join("runtime"));
    let p = t.path().join("good.png");
    image::RgbImage::new(40, 30).save(&p).unwrap();
    let (folder, id) = register(&s, &p);
    s.allowed
        .lock()
        .unwrap()
        .insert(folder, t.path().canonicalize().unwrap());
    fs::write(
        s.data.join("cache").join(format!(
            "{id}-{}-480.jpg",
            imaging::fingerprint(&p).unwrap()
        )),
        b"old unmanaged cache",
    )
    .unwrap();
    assert!(image::load_from_memory(&imaging::render(&s, &id, 480).unwrap()).is_ok());
}

#[test]
fn bundled_sqlite_includes_the_wal_page_size_security_fix() {
    assert!(
        rusqlite::version_number() >= 3_054_000,
        "Bundled SQLite must include the RUSTSEC-2026-0459 fix"
    );
}
