//! Bounded, orientation-aware SDR previews using the system colour pipeline.
use crate::catalog::Result;
use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    data::{CFData, CFDataCreateMutable, CFDataRef, CFMutableDataRef},
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
    url::CFURL,
};
use core_graphics::{
    base::{kCGBitmapByteOrder32Big, kCGImageAlphaPremultipliedLast},
    color_space::{CGColorSpace, kCGColorSpaceSRGB},
    context::CGContext,
    geometry::{CGAffineTransform, CGPoint, CGRect, CGSize},
    image::CGImage,
};
use foreign_types::ForeignType;
use std::{ffi::c_void, path::Path, ptr};
type Object = *const c_void;
type Dict = CFDictionary<CFString, CFType>;
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithURL(
        url: core_foundation::url::CFURLRef,
        options: CFDictionaryRef,
    ) -> Object;
    fn CGImageSourceGetCount(source: Object) -> usize;
    fn CGImageSourceGetType(source: Object) -> CFStringRef;
    fn CGImageSourceCopyPropertiesAtIndex(
        source: Object,
        index: usize,
        options: CFDictionaryRef,
    ) -> CFDictionaryRef;
    fn CGImageSourceCopyAuxiliaryDataInfoAtIndex(
        source: Object,
        index: usize,
        kind: CFStringRef,
    ) -> CFDictionaryRef;
    fn CGImageSourceCreateThumbnailAtIndex(
        source: Object,
        index: usize,
        options: CFDictionaryRef,
    ) -> *mut core_graphics::sys::CGImage;
    fn CGImageDestinationCreateWithData(
        data: CFMutableDataRef,
        kind: CFStringRef,
        count: usize,
        options: CFDictionaryRef,
    ) -> Object;
    fn CGImageDestinationAddImage(
        dest: Object,
        image: *mut core_graphics::sys::CGImage,
        properties: CFDictionaryRef,
    );
    fn CGImageDestinationFinalize(dest: Object) -> bool;
    static kCGImageSourceShouldCache: CFStringRef;
    static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
    static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
    static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
    static kCGImageAuxiliaryDataTypeHDRGainMap: CFStringRef;
    static kCGImageDestinationLossyCompressionQuality: CFStringRef;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        row_bytes: usize,
        space: *mut core_graphics::sys::CGColorSpace,
        info: u32,
    ) -> *mut core_graphics::sys::CGContext;
}
fn owned(raw: Object) -> Result<CFType> {
    if raw.is_null() {
        return Err("图片损坏或系统暂不支持此格式".into());
    }
    Ok(unsafe { CFType::wrap_under_create_rule(raw) })
}
fn key(raw: CFStringRef) -> CFString {
    unsafe { CFString::wrap_under_get_rule(raw) }
}
fn number(d: &Dict, k: &str) -> Option<i64> {
    d.find(CFString::new(k))?.downcast::<CFNumber>()?.to_i64()
}
// ImageIO's thumbnail transform also corrects non-square print density. A photo's
// pixel canvas must retain its aspect ratio, independently of its print DPI.
pub(crate) fn unequal_density(props: &Dict) -> bool {
    let dpi = |key| {
        props
            .find(CFString::new(key))?
            .downcast::<CFNumber>()?
            .to_f64()
    };
    matches!((dpi("DPIWidth"), dpi("DPIHeight")), (Some(x), Some(y)) if x != y)
}

pub(crate) fn draw_oriented(context: &CGContext, rect: CGRect, image: &CGImage, orientation: i64) {
    let (a, b, c, d, tx, ty) = match orientation {
        2 => (-1., 0., 0., 1., 1., 0.),
        3 => (-1., 0., 0., -1., 1., 1.),
        4 => (1., 0., 0., -1., 0., 1.),
        5 => (0., -1., -1., 0., 1., 1.),
        6 => (0., -1., 1., 0., 0., 1.),
        7 => (0., 1., 1., 0., 0., 0.),
        8 => (0., 1., -1., 0., 1., 0.),
        _ => (1., 0., 0., 1., 0., 0.),
    };
    context.save();
    context.translate(rect.origin.x, rect.origin.y);
    context.scale(rect.size.width, rect.size.height);
    context.concat_ctm(CGAffineTransform::new(a, b, c, d, tx, ty));
    context.draw_image(
        CGRect::new(&CGPoint::new(0., 0.), &CGSize::new(1., 1.)),
        image,
    );
    context.restore();
}
// Resolve newer ImageIO constants at runtime: importing them strongly would
// prevent ordinary SDR browsing from launching on the declared older systems.
fn optional_string(symbol: &std::ffi::CStr) -> Option<CFString> {
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
    }
    let address = unsafe { dlsym((-2isize) as *mut c_void, symbol.as_ptr()) };
    if address.is_null() {
        return None;
    }
    let value = unsafe { *(address as *const CFStringRef) };
    if value.is_null() {
        None
    } else {
        Some(key(value))
    }
}
fn check_sdr_policy(kind: &str, depth: i64, hdr: bool, available: bool) -> Result<()> {
    if depth > 16 || (depth > 8 && !["public.heic", "public.heif"].contains(&kind)) {
        return Err(
            "暂不转换此高位深格式；支持范围为静态 JPEG、HEIC/HEIF 的已验证 SDR 解码".into(),
        );
    }
    if hdr && !["public.jpeg", "public.heic", "public.heif"].contains(&kind) {
        return Err("此HDR格式尚未验证，未转换；请保留原件".into());
    }
    if hdr && !available {
        return Err("此 HDR 照片需要 macOS 14 或更新系统的 SDR 解码能力；原件保持不变".into());
    }
    Ok(())
}
pub(crate) fn sdr_options(source: &CFType, props: &Dict) -> Result<Vec<(CFString, CFType)>> {
    let raw_kind = unsafe { CGImageSourceGetType(source.as_CFTypeRef()) };
    if raw_kind.is_null() {
        return Err("无法识别照片格式".into());
    }
    let kind = key(raw_kind).to_string();
    if ![
        "public.jpeg",
        "public.png",
        "org.webmproject.webp",
        "public.webp",
        "public.heic",
        "public.heif",
    ]
    .contains(&kind.as_str())
    {
        return Err("仅转换静态 JPEG、PNG、WebP 和 HEIC/HEIF".into());
    }
    let mut gain = false;
    let mut types = vec![key(unsafe { kCGImageAuxiliaryDataTypeHDRGainMap })];
    if let Some(iso) = optional_string(c"kCGImageAuxiliaryDataTypeISOGainMap") {
        types.push(iso);
    }
    for t in types {
        let raw = unsafe {
            CGImageSourceCopyAuxiliaryDataInfoAtIndex(
                source.as_CFTypeRef(),
                0,
                t.as_concrete_TypeRef(),
            )
        };
        if !raw.is_null() {
            let _owned = unsafe { Dict::wrap_under_create_rule(raw) };
            gain = true;
        }
    }
    let profile = props
        .find(CFString::new("ProfileName"))
        .and_then(|v| v.downcast::<CFString>())
        .map(|s| s.to_string().to_lowercase())
        .unwrap_or_default();
    let depth = number(props, "Depth").unwrap_or(8);
    let hdr = gain
        || depth > 8
        || ["2100", "hdr", "pq", "hlg", "linear"]
            .iter()
            .any(|p| profile.contains(p));
    let decode = optional_string(c"kCGImageSourceDecodeRequest")
        .zip(optional_string(c"kCGImageSourceDecodeToSDR"));
    check_sdr_policy(&kind, depth, hdr, decode.is_some())?;
    Ok(decode
        .map(|(key, value)| vec![(key, value.as_CFType())])
        .unwrap_or_default())
}

pub(crate) fn header(path: &Path) -> Result<(u32, u32, Option<String>)> {
    let url = CFURL::from_path(path, false).ok_or("照片路径无效")?;
    let options = Dict::from_CFType_pairs(&[(
        key(unsafe { kCGImageSourceShouldCache }),
        CFBoolean::false_value().as_CFType(),
    )]);
    let source = owned(unsafe {
        CGImageSourceCreateWithURL(url.as_concrete_TypeRef(), options.as_concrete_TypeRef())
    })?;
    let raw = unsafe { CGImageSourceCopyPropertiesAtIndex(source.as_CFTypeRef(), 0, ptr::null()) };
    if raw.is_null() {
        return Err("照片元数据无法读取".into());
    }
    let p = unsafe { Dict::wrap_under_create_rule(raw) };
    let width = number(&p, "PixelWidth")
        .and_then(|v| u32::try_from(v).ok())
        .ok_or("照片宽度无效")?;
    let height = number(&p, "PixelHeight")
        .and_then(|v| u32::try_from(v).ok())
        .ok_or("照片高度无效")?;
    let orientation = number(&p, "Orientation").unwrap_or(1);
    if !(1..=8).contains(&orientation) {
        return Err("照片方向无效".into());
    }
    let date = p
        .find(CFString::new("{Exif}"))
        .and_then(|v| v.downcast::<CFDictionary>())
        .map(|d| unsafe { Dict::wrap_under_get_rule(d.as_concrete_TypeRef()) })
        .and_then(|d| {
            d.find(CFString::new("DateTimeOriginal"))
                .and_then(|v| v.downcast::<CFString>())
                .map(|v| v.to_string())
        });
    Ok(if orientation >= 5 {
        (height, width, date)
    } else {
        (width, height, date)
    })
}

pub fn preview(path: &Path, size: u32) -> Result<Vec<u8>> {
    let url = CFURL::from_path(path, false).ok_or("照片路径无效")?;
    let options = Dict::from_CFType_pairs(&[(
        key(unsafe { kCGImageSourceShouldCache }),
        CFBoolean::false_value().as_CFType(),
    )]);
    let source = owned(unsafe {
        CGImageSourceCreateWithURL(url.as_concrete_TypeRef(), options.as_concrete_TypeRef())
    })?;
    let kind = unsafe { CGImageSourceGetType(source.as_CFTypeRef()) };
    if kind.is_null()
        || ![
            "public.jpeg",
            "public.png",
            "org.webmproject.webp",
            "public.webp",
            "public.heic",
            "public.heif",
        ]
        .contains(&key(kind).to_string().as_str())
    {
        return Err("首版仅预览 JPEG、PNG、WebP 和单帧 SDR HEIC/HEIF".into());
    }
    if unsafe { CGImageSourceGetCount(source.as_CFTypeRef()) } != 1 {
        return Err("多帧或配对照片暂不预览，请保留完整资源组".into());
    }
    let raw = unsafe { CGImageSourceCopyPropertiesAtIndex(source.as_CFTypeRef(), 0, ptr::null()) };
    if raw.is_null() {
        return Err("照片元数据无法读取".into());
    }
    let props = unsafe { Dict::wrap_under_create_rule(raw) };
    let width = number(&props, "PixelWidth").ok_or("照片宽度无效")?;
    let height = number(&props, "PixelHeight").ok_or("照片高度无效")?;
    if width <= 0 || height <= 0 || width > 30000 || height > 30000 || width * height > 64_000_000 {
        return Err("预览限6400万像素且边长不超过30000px".into());
    }
    let decode = sdr_options(&source, &props)?;
    let orientation = number(&props, "Orientation").unwrap_or(1);
    if !(1..=8).contains(&orientation) {
        return Err("照片方向信息无效".into());
    }
    let max = i64::from(size).min(width.max(height));
    // One detail pane is allowed; its RGBA bitmap still has a hard pixel budget.
    let ratio = max as f64 / width.max(height) as f64;
    if width as f64 * height as f64 * ratio * ratio > 24_000_000. {
        return Err("细节预览超过2400万像素，请使用普通预览".into());
    }
    let manual = unequal_density(&props);
    let mut attrs = vec![
        (
            key(unsafe { kCGImageSourceCreateThumbnailFromImageAlways }),
            CFBoolean::true_value().as_CFType(),
        ),
        (
            key(unsafe { kCGImageSourceCreateThumbnailWithTransform }),
            CFBoolean::from(!manual).as_CFType(),
        ),
        (
            key(unsafe { kCGImageSourceThumbnailMaxPixelSize }),
            CFNumber::from(max).as_CFType(),
        ),
    ];
    attrs.extend(decode);
    let attrs = Dict::from_CFType_pairs(&attrs);
    let raw = unsafe {
        CGImageSourceCreateThumbnailAtIndex(source.as_CFTypeRef(), 0, attrs.as_concrete_TypeRef())
    };
    if raw.is_null() {
        return Err("系统无法解码这张照片，可重试或跳过".into());
    }
    let image = unsafe { CGImage::from_ptr(raw) };
    let (w, h) = if manual && orientation >= 5 {
        (image.height(), image.width())
    } else {
        (image.width(), image.height())
    };
    if image.bits_per_component() > 8 {
        return Err("系统未返回8位SDR预览，未显示未经验证的转换".into());
    }
    if w == 0
        || h == 0
        || w > max as usize
        || h > max as usize
        || w.checked_mul(h).is_none_or(|n| n > 24_000_000)
    {
        return Err("系统预览尺寸超过限制".into());
    }
    let space =
        CGColorSpace::create_with_name(unsafe { kCGColorSpaceSRGB }).ok_or("sRGB色彩空间不可用")?;
    let raw = unsafe {
        CGBitmapContextCreate(
            ptr::null_mut(),
            w,
            h,
            8,
            w * 4,
            space.as_ptr(),
            kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
        )
    };
    if raw.is_null() {
        return Err("预览内存不足，请稍后重试".into());
    }
    let context = unsafe { CGContext::from_ptr(raw) };
    let rect = CGRect::new(&CGPoint::new(0., 0.), &CGSize::new(w as f64, h as f64));
    context.set_rgb_fill_color(1., 1., 1., 1.);
    context.fill_rect(rect);
    if manual {
        draw_oriented(&context, rect, &image, orientation);
    } else {
        context.draw_image(rect, &image);
    }
    let output = context.create_image().ok_or("无法生成预览")?;
    let raw = unsafe { CFDataCreateMutable(ptr::null(), 0) };
    if raw.is_null() {
        return Err("无法分配预览输出".into());
    }
    let data = unsafe { CFData::wrap_under_create_rule(raw as CFDataRef) };
    let kind = CFString::new("public.jpeg");
    let dest = owned(unsafe {
        CGImageDestinationCreateWithData(raw, kind.as_concrete_TypeRef(), 1, ptr::null())
    })?;
    let exif = Dict::from_CFType_pairs(&[(
        CFString::new("ColorSpace"),
        CFNumber::from(1i32).as_CFType(),
    )]);
    let props = Dict::from_CFType_pairs(&[
        (
            key(unsafe { kCGImageDestinationLossyCompressionQuality }),
            CFNumber::from(0.88).as_CFType(),
        ),
        (CFString::new("{Exif}"), exif.as_CFType()),
    ]);
    unsafe {
        CGImageDestinationAddImage(
            dest.as_CFTypeRef(),
            output.as_ptr(),
            props.as_concrete_TypeRef(),
        );
    }
    if !unsafe { CGImageDestinationFinalize(dest.as_CFTypeRef()) } {
        return Err("预览编码失败".into());
    }
    Ok(data.bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::preview;
    #[test]
    fn hdr_policy_keeps_old_systems_and_unknown_high_depth_safe() {
        use super::check_sdr_policy;
        assert!(check_sdr_policy("public.jpeg", 8, false, false).is_ok());
        assert!(
            check_sdr_policy("public.jpeg", 8, true, false)
                .unwrap_err()
                .contains("macOS 14")
        );
        assert!(check_sdr_policy("public.heic", 10, true, true).is_ok());
        assert!(check_sdr_policy("public.png", 16, true, true).is_err());
        assert!(check_sdr_policy("public.png", 8, true, true).is_err());
        assert!(check_sdr_policy("public.heic", 32, true, true).is_err());
    }

    #[test]
    fn unequal_print_density_preserves_pixels_and_all_exif_orientations() {
        use image::{GenericImageView, metadata::Orientation};
        let temp = tempfile::tempdir().unwrap();
        for orientation in 1..=8 {
            let mut bytes = crate::watermark::tests::fixture(
                640,
                480,
                orientation,
                Some("2026:10:25 23:59:59"),
            );
            let jfif = bytes.windows(5).position(|b| b == b"JFIF\0").unwrap();
            bytes[jfif + 7] = 1;
            bytes[jfif + 8..jfif + 10].copy_from_slice(&80u16.to_be_bytes());
            bytes[jfif + 10..jfif + 12].copy_from_slice(&40u16.to_be_bytes());
            let path = temp.path().join(format!("orientation-{orientation}.jpg"));
            std::fs::write(&path, &bytes).unwrap();
            let mut expected = image::load_from_memory(&bytes).unwrap();
            expected.apply_orientation(Orientation::from_exif(orientation as u8).unwrap());
            let watermarked =
                crate::watermark::render(&bytes, &crate::watermark::Options::default(), None)
                    .unwrap()
                    .jpeg;
            for output in [preview(&path, 1600).unwrap(), watermarked] {
                let actual = image::load_from_memory(&output).unwrap();
                assert_eq!(
                    actual.dimensions(),
                    expected.dimensions(),
                    "orientation {orientation}"
                );
                let (w, h) = actual.dimensions();
                for (x, y) in [(8, 8), (w - 9, 8), (8, h - 9), (w - 9, h - 9)] {
                    let a = actual.get_pixel(x, y);
                    let b = expected.get_pixel(x, y);
                    for channel in 0..3 {
                        assert!(
                            (i32::from(a[channel]) - i32::from(b[channel])).abs() < 25,
                            "orientation {orientation}, corner {x},{y}"
                        );
                    }
                }
            }
        }
    }
}
