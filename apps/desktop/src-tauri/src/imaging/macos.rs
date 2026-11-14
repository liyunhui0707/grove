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
    geometry::{CGPoint, CGRect, CGSize},
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
    let profile = props
        .find(CFString::new("ProfileName"))
        .and_then(|v| v.downcast::<CFString>())
        .map(|v| v.to_string().to_lowercase());
    if number(&props, "Depth").is_some_and(|n| n > 8)
        || profile.is_some_and(|s| {
            ["2100", "hdr", "pq", "hlg", "linear"]
                .iter()
                .any(|k| s.contains(k))
        })
    {
        return Err("HDR/高位深照片暂不预览；原件保持不变".into());
    }
    let gain = unsafe {
        CGImageSourceCopyAuxiliaryDataInfoAtIndex(
            source.as_CFTypeRef(),
            0,
            kCGImageAuxiliaryDataTypeHDRGainMap,
        )
    };
    if !gain.is_null() {
        let _gain = unsafe { Dict::wrap_under_create_rule(gain) };
        return Err("检测到HDR增益图，暂不预览；原件保持不变".into());
    }
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
    let attrs = Dict::from_CFType_pairs(&[
        (
            key(unsafe { kCGImageSourceCreateThumbnailFromImageAlways }),
            CFBoolean::true_value().as_CFType(),
        ),
        (
            key(unsafe { kCGImageSourceCreateThumbnailWithTransform }),
            CFBoolean::true_value().as_CFType(),
        ),
        (
            key(unsafe { kCGImageSourceThumbnailMaxPixelSize }),
            CFNumber::from(max).as_CFType(),
        ),
    ]);
    let raw = unsafe {
        CGImageSourceCreateThumbnailAtIndex(source.as_CFTypeRef(), 0, attrs.as_concrete_TypeRef())
    };
    if raw.is_null() {
        return Err("系统无法解码这张照片，可重试或跳过".into());
    }
    let image = unsafe { CGImage::from_ptr(raw) };
    let (w, h) = (image.width(), image.height());
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
    context.draw_image(rect, &image);
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
