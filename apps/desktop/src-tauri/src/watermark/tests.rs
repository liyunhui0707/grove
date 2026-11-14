use super::*;

pub(crate) fn fixture(width: u32, height: u32, orientation: u16, date: Option<&str>) -> Vec<u8> {
    use image::{Rgb, RgbImage, codecs::jpeg::JpegEncoder};
    let image = RgbImage::from_fn(width, height, |x, y| {
        Rgb(match (x < width / 2, y < height / 2) {
            (true, true) => [200, 40, 30],
            (false, true) => [20, 170, 70],
            (true, false) => [30, 70, 200],
            _ => [125, 125, 125],
        })
    });
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 95)
        .encode_image(&image)
        .unwrap();
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    let entries = if date.is_some() { 3u16 } else { 2 };
    let first_end = 8 + 2 + u32::from(entries) * 12 + 4;
    let gps_offset = if date.is_some() {
        (first_end + 57).next_multiple_of(2)
    } else {
        first_end
    };
    tiff.extend(entries.to_le_bytes());
    {
        let (tag, kind, count, value) = (0x0112u16, 3u16, 1u32, u32::from(orientation));
        tiff.extend(tag.to_le_bytes());
        tiff.extend(kind.to_le_bytes());
        tiff.extend(count.to_le_bytes());
        tiff.extend(value.to_le_bytes());
    }
    if date.is_some() {
        tiff.extend(0x8769u16.to_le_bytes());
        tiff.extend(4u16.to_le_bytes());
        tiff.extend(1u32.to_le_bytes());
        tiff.extend(first_end.to_le_bytes());
    }
    tiff.extend(0x8825u16.to_le_bytes());
    tiff.extend(4u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(gps_offset.to_le_bytes());
    tiff.extend([0; 4]);
    if let Some(date) = date {
        tiff.extend(2u16.to_le_bytes());
        for (tag, len, offset) in [
            (0x9003u16, 20u32, first_end + 30),
            (0x9011, 7, first_end + 50),
        ] {
            tiff.extend(tag.to_le_bytes());
            tiff.extend(2u16.to_le_bytes());
            tiff.extend(len.to_le_bytes());
            tiff.extend(offset.to_le_bytes());
        }
        tiff.extend([0; 4]);
        assert_eq!(date.len(), 19);
        tiff.extend(date.as_bytes());
        tiff.push(0);
        tiff.extend(b"+14:00\0");
    }
    while tiff.len() < gps_offset as usize {
        tiff.push(0);
    }
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(2u32.to_le_bytes());
    tiff.extend(b"N\0\0\0");
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(5u16.to_le_bytes());
    tiff.extend(3u32.to_le_bytes());
    tiff.extend((gps_offset + 30).to_le_bytes());
    tiff.extend([0; 4]);
    for n in [28u32, 0, 0] {
        tiff.extend(n.to_le_bytes());
        tiff.extend(1u32.to_le_bytes());
    }
    let mut packet = b"Exif\0\0".to_vec();
    packet.extend(tiff);
    let mut result = jpeg[..2].to_vec();
    result.extend([0xff, 0xe1]);
    result.extend(((packet.len() + 2) as u16).to_be_bytes());
    result.extend(packet);
    result.extend(&jpeg[2..]);
    result
}
#[test]
fn dates_keep_camera_day_and_reject_invented_or_invalid_values() {
    assert_eq!(exif_date("2026:10:25 23:59:58"), Some("2026-10-25".into()));
    for s in [
        "2026:02:29 12:00:00",
        "2026:10:25 24:00:00",
        "0000:00:00 00:00:00",
        "2026-10-25",
        "bad",
    ] {
        assert!(exif_date(s).is_none());
    }
    assert!(valid_date("2024-02-29"));
    assert!(!valid_date("1900-02-29"));
    let o = Options {
        city: " 温州 ".into(),
        fallback_date: Some("2026-11-09".into()),
        ..Options::default()
    }
    .normalized()
    .unwrap();
    let info = caption(1200, 800, Some("2026-10-25".into()), None, &o).unwrap();
    assert_eq!(info.text, "2026-10-25 温州");
    assert_eq!(info.date_source, "exif");
    let info = caption(800, 1200, None, None, &o).unwrap();
    assert_eq!(info.text, "2026-11-09 温州");
    assert_eq!(info.date_source, "manual");
    let info = caption(800, 1200, None, None, &Options::default()).unwrap();
    assert!(info.date.is_none());
    assert!(info.text.is_empty());
}
#[test]
fn layout_options_are_bounded_and_do_not_accept_control_text() {
    for o in [
        Options {
            city: "温州\n地址".into(),
            ..Options::default()
        },
        Options {
            color: "red".into(),
            ..Options::default()
        },
        Options {
            opacity: f64::NAN,
            ..Options::default()
        },
        Options {
            fallback_date: Some("2026-13-01".into()),
            ..Options::default()
        },
    ] {
        assert!(o.normalized().is_err());
    }
    assert!(caption(12001, 100, None, None, &Options::default()).is_err());
    assert!(caption(6000, 5000, None, None, &Options::default()).is_err());
}

#[test]
#[cfg(target_os = "macos")]
fn native_dates_orientation_and_chinese_watermark_stay_inside_original_canvas() {
    use image::{GenericImageView, metadata::Orientation};
    let options = Options {
        city: "温州".into(),
        ..Options::default()
    };
    for orientation in 1..=8 {
        let bytes = fixture(640, 480, orientation, Some("2026:10:25 23:59:59"));
        let original = bytes.clone();
        let info = inspect(&bytes, &options).unwrap();
        assert_eq!(info.date.as_deref(), Some("2026-10-25"));
        assert_eq!(info.date_source, "exif");
        let rendered = render(&bytes, &options, None).unwrap();
        assert_eq!(rendered.info.source, info);
        assert_eq!(bytes, original);
        let output = image::load_from_memory(&rendered.jpeg).unwrap();
        let mut expected = image::load_from_memory(&bytes).unwrap();
        expected.apply_orientation(Orientation::from_exif(orientation as u8).unwrap());
        assert_eq!(output.dimensions(), expected.dimensions());
        let (w, h) = output.dimensions();
        for (x, y) in [(8, 8), (w - 9, 8), (8, h - 9), (w - 9, h - 9)] {
            let a = output.get_pixel(x, y);
            let b = expected.get_pixel(x, y);
            for k in 0..3 {
                assert!(
                    (i32::from(a[k]) - i32::from(b[k])).abs() < 25,
                    "orientation {orientation} corner {x},{y}"
                );
            }
        }
        let bounds = rendered.info.bounds.unwrap();
        assert!(
            bounds.x >= 0.
                && bounds.y >= 0.
                && bounds.x + bounds.width <= f64::from(w)
                && bounds.y + bounds.height <= f64::from(h)
        );
        assert!(bounds.x > f64::from(w) / 2. && bounds.y > f64::from(h) / 2.);
        // A fresh pixel image carries no original date/GPS metadata into the derivative.
        let derivative = inspect(&rendered.jpeg, &Options::default()).unwrap();
        assert!(derivative.date.is_none());
        assert!(
            derivative
                .source_profile
                .as_deref()
                .is_some_and(|p| p.to_lowercase().contains("srgb"))
        );
        let input_exif = exif::Reader::new()
            .read_from_container(&mut std::io::Cursor::new(&bytes))
            .unwrap();
        assert!(
            input_exif
                .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                .is_some()
        );
        let output_exif =
            exif::Reader::new().read_from_container(&mut std::io::Cursor::new(&rendered.jpeg));
        assert!(output_exif.ok().is_none_or(|e| {
            e.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                .is_none()
        }));
    }
}

#[test]
#[cfg(target_os = "macos")]
fn native_png_and_generated_sdr_heic_have_independent_jpeg_outputs() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let image = image::RgbaImage::from_pixel(640, 480, image::Rgba([0, 0, 0, 0]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let options = Options {
        fallback_date: Some("2026-10-25".into()),
        ..Options::default()
    };
    let output = render(&png, &options, None).unwrap();
    let pixels = image::load_from_memory(&output.jpeg).unwrap().to_rgb8();
    assert!(pixels.get_pixel(5, 5).0.iter().all(|c| *c > 245));
    let high_depth = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_pixel(
        32,
        24,
        image::Rgb([65535, 32768, 0]),
    );
    let mut png16 = Vec::new();
    image::DynamicImage::ImageRgb16(high_depth)
        .write_to(
            &mut std::io::Cursor::new(&mut png16),
            image::ImageFormat::Png,
        )
        .unwrap();
    assert!(inspect(&png16, &options).unwrap_err().contains("高位深"));
    let (input, heic) = (root.join("generated.jpg"), root.join("generated.heic"));
    let bytes = fixture(640, 480, 6, Some("2026:10:25 23:59:59"));
    std::fs::write(&input, &bytes).unwrap();
    let converted = std::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "heic"])
        .arg(&input)
        .arg("--out")
        .arg(&heic)
        .output()
        .unwrap();
    assert!(converted.status.success());
    let bytes = std::fs::read(&heic).unwrap();
    let output = render(
        &bytes,
        &Options {
            city: "温州".into(),
            ..Options::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(output.info.source.date.as_deref(), Some("2026-10-25"));
    assert_eq!(
        (output.info.output_width, output.info.output_height),
        (480, 640)
    );
    assert_eq!(std::fs::read(&heic).unwrap(), bytes);
}

#[test]
#[cfg(target_os = "macos")]
fn preview_and_export_share_layout_and_missing_dates_require_explicit_fallback() {
    let bytes = fixture(1600, 1200, 1, None);
    let options = Options {
        city: "温州".into(),
        ..Options::default()
    };
    assert!(render(&bytes, &options, None).is_err());
    let preview = render(&bytes, &options, Some(640)).unwrap();
    assert!(preview.info.source.date.is_none());
    assert!(preview.info.bounds.is_none());
    let options = Options {
        fallback_date: Some("2026-10-25".into()),
        ..options
    };
    let a = render(&bytes, &options, Some(640)).unwrap();
    let b = render(&bytes, &options, None).unwrap();
    assert_eq!(a.info.source, b.info.source);
    assert_eq!(a.info.bounds, b.info.bounds);
    assert_eq!(a.info.font_size, b.info.font_size);
    assert_eq!(b.info.source.date_source, "manual");
    let image = image::load_from_memory(&b.jpeg).unwrap().to_rgb8();
    let white = image
        .enumerate_pixels()
        .filter(|(_, _, p)| p[0] > 200 && p[1] > 200 && p[2] > 200)
        .collect::<Vec<_>>();
    assert!(white.len() > 30);
    assert!(white.iter().all(|(x, y, _)| *x > 800 && *y > 600));
    let serif = Options {
        font: "serif".into(),
        color: "#253728".into(),
        opacity: 0.5,
        ..options
    };
    assert!(render(&bytes, &serif, None).unwrap().info.bounds.is_some());
}

#[test]
fn bounded_file_input_checks_size_and_uses_the_open_file() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("generated-input");
    std::fs::write(&path, b"original input").unwrap();
    let original = std::fs::File::open(&path).unwrap();
    std::fs::rename(&path, t.path().join("kept-original")).unwrap();
    std::fs::write(&path, b"different occurrence").unwrap();
    assert_eq!(read_input(original).unwrap(), b"original input");
    let f = std::fs::File::create(&path).unwrap();
    assert!(read_input(f.try_clone().unwrap()).is_err());
    f.set_len(MAX_INPUT_BYTES + 1).unwrap();
    assert!(read_input(std::fs::File::open(&path).unwrap()).is_err());
    f.set_len(MAX_INPUT_BYTES).unwrap();
    assert_eq!(
        read_input(std::fs::File::open(&path).unwrap())
            .unwrap()
            .len() as u64,
        MAX_INPUT_BYTES
    );
}
