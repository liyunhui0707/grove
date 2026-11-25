//! System ImageIO/CoreText, using the installed SDK's public C interfaces.
//! Every create/copy object is null-checked and owned until its last dependent call.
use super::*;
use core_foundation::{
    array::{CFArray, CFArrayRef},
    base::{CFRange, CFType, TCFType},
    boolean::CFBoolean,
    data::{CFData, CFDataCreateMutable, CFDataRef, CFMutableDataRef},
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    base::{kCGBitmapByteOrder32Big, kCGImageAlphaPremultipliedLast},
    color_space::{CGColorSpace, kCGColorSpaceSRGB},
    context::CGContext,
    geometry::{CGPoint, CGRect, CGSize},
    image::CGImage,
};
use foreign_types::ForeignType;
use std::{ffi::c_void, ptr};
type Object = *const c_void;
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
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: CFDataRef, options: CFDictionaryRef) -> Object;
    fn CGImageSourceCopyPropertiesAtIndex(
        source: Object,
        index: usize,
        options: CFDictionaryRef,
    ) -> CFDictionaryRef;
    fn CGImageSourceCreateThumbnailAtIndex(
        source: Object,
        index: usize,
        options: CFDictionaryRef,
    ) -> *mut core_graphics::sys::CGImage;
    fn CGImageSourceGetCount(source: Object) -> usize;
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
    static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
    static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
    static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
    static kCGImageSourceShouldCache: CFStringRef;
    static kCGImageDestinationLossyCompressionQuality: CFStringRef;
}
#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    fn CTFontCreateUIFontForLanguage(kind: u32, size: f64, language: CFStringRef) -> Object;
    fn CTFontCreateWithName(name: CFStringRef, size: f64, matrix: *const c_void) -> Object;
    fn CTLineCreateWithAttributedString(text: Object) -> Object;
    fn CTLineGetImageBounds(line: Object, context: *mut core_graphics::sys::CGContext) -> CGRect;
    fn CTLineDraw(line: Object, context: *mut core_graphics::sys::CGContext);
    fn CTLineGetGlyphRuns(line: Object) -> CFArrayRef;
    fn CTRunGetGlyphCount(run: Object) -> isize;
    fn CTRunGetGlyphs(run: Object, range: CFRange, buffer: *mut u16);
    static kCTFontAttributeName: CFStringRef;
    static kCTForegroundColorFromContextAttributeName: CFStringRef;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFAttributedStringCreate(
        allocator: Object,
        text: CFStringRef,
        attributes: CFDictionaryRef,
    ) -> Object;
}
type Dict = CFDictionary<CFString, CFType>;
fn owned(raw: Object) -> Result<CFType> {
    if raw.is_null() {
        return Err("系统图像/文字对象创建失败".into());
    }
    Ok(unsafe { CFType::wrap_under_create_rule(raw) })
}
fn key(raw: CFStringRef) -> CFString {
    unsafe { CFString::wrap_under_get_rule(raw) }
}
fn value(d: &Dict, k: &str) -> Option<CFType> {
    d.find(CFString::new(k)).map(|v| (*v).clone())
}
fn string(d: &Dict, k: &str) -> Option<String> {
    value(d, k)?.downcast::<CFString>().map(|s| s.to_string())
}
fn number(d: &Dict, k: &str) -> Option<i64> {
    value(d, k)?.downcast::<CFNumber>()?.to_i64()
}
fn source(bytes: &[u8]) -> Result<(CFData, CFType, Dict)> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("首版水印限单张32MiB以内".into());
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        crate::imaging::check_png_header(std::io::Cursor::new(bytes))?;
    }
    let data = CFData::from_buffer(bytes);
    let options = CFDictionary::from_CFType_pairs(&[(
        key(unsafe { kCGImageSourceShouldCache }),
        CFBoolean::false_value().as_CFType(),
    )]);
    let source = owned(unsafe {
        CGImageSourceCreateWithData(data.as_concrete_TypeRef(), options.as_concrete_TypeRef())
    })?;
    if unsafe { CGImageSourceGetCount(source.as_CFTypeRef()) } != 1 {
        return Err("首版水印仅处理单帧静态照片".into());
    }
    let raw = unsafe { CGImageSourceCopyPropertiesAtIndex(source.as_CFTypeRef(), 0, ptr::null()) };
    if raw.is_null() {
        return Err("无法读取照片元数据".into());
    }
    let props = unsafe { Dict::wrap_under_create_rule(raw) };
    Ok((data, source, props))
}
fn nested(props: &Dict, key: &str) -> Option<Dict> {
    let dict = value(props, key)?.downcast::<CFDictionary>()?;
    Some(unsafe { Dict::wrap_under_get_rule(dict.as_concrete_TypeRef()) })
}

fn coordinate(gps: &Dict, axis: &str, positive: &str, negative: &str) -> Option<f64> {
    let n = value(gps, axis)?.downcast::<CFNumber>()?.to_f64()?;
    match string(gps, &format!("{axis}Ref"))?.as_str() {
        d if d == positive && n >= 0.0 => Some(n),
        d if d == negative => Some(-n.abs()),
        _ => None,
    }
}

fn detect_city(p: &Dict) -> Option<String> {
    if let Some(city) = nested(p, "{GPS}").and_then(|gps| {
        let lat = coordinate(&gps, "Latitude", "N", "S")?;
        let lon = coordinate(&gps, "Longitude", "E", "W")?;
        super::cities::nearby(lat, lon)
    }) {
        return Some(city.to_owned());
    }
    let raw = string(&nested(p, "{IPTC}")?, "City")?;
    let city = raw.trim();
    (!city.is_empty() && super::valid_city(city)).then(|| city.to_owned())
}

fn metadata(source: &CFType, p: &Dict, o: &Options, planned: Option<&Info>) -> Result<Info> {
    let width = number(p, "PixelWidth")
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("照片宽度无效")?;
    let height = number(p, "PixelHeight")
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("照片高度无效")?;
    let orientation = number(p, "Orientation").unwrap_or(1);
    if !(1..=8).contains(&orientation) {
        return Err("照片方向信息无效".into());
    }
    let (w, h) = if orientation >= 5 {
        (height, width)
    } else {
        (width, height)
    };
    let profile = string(p, "ProfileName");
    let date = value(p, "{Exif}")
        .and_then(|v| v.downcast::<CFDictionary>())
        .map(|d| unsafe { Dict::wrap_under_get_rule(d.as_concrete_TypeRef()) })
        .and_then(|d| string(&d, "DateTimeOriginal"))
        .and_then(|s| exif_date(&s));
    let mut effective = o.clone();
    let mut detected_city = None;
    if o.city_overrides.is_some() {
        detected_city = match planned {
            Some(info) => info.detected_city.clone(),
            None => detect_city(p),
        };
        effective.city = detected_city.clone().unwrap_or_default();
    }
    let mut info = caption(w, h, date, profile, &effective)?;
    info.detected_city = detected_city;
    crate::imaging::sdr_options(source, p)?;
    Ok(info)
}
pub fn inspect(bytes: &[u8], options: &Options) -> Result<Info> {
    let o = options.normalized()?;
    let (_data, source, props) = source(bytes)?;
    metadata(&source, &props, &o, None)
}
fn line(text: &str, size: f64, font: &str) -> Result<CFType> {
    let language = CFString::new("zh-Hans");
    let font = owned(unsafe {
        if font == "serif" {
            CTFontCreateWithName(
                CFString::new("TimesNewRomanPSMT").as_concrete_TypeRef(),
                size,
                ptr::null(),
            )
        } else {
            CTFontCreateUIFontForLanguage(2, size, language.as_concrete_TypeRef())
        }
    })?;
    let attrs = CFDictionary::from_CFType_pairs(&[
        (key(unsafe { kCTFontAttributeName }), font),
        (
            key(unsafe { kCTForegroundColorFromContextAttributeName }),
            CFBoolean::true_value().as_CFType(),
        ),
    ]);
    let text = CFString::new(text);
    let attributed = owned(unsafe {
        CFAttributedStringCreate(
            ptr::null(),
            text.as_concrete_TypeRef(),
            attrs.as_concrete_TypeRef(),
        )
    })?;
    let line = owned(unsafe { CTLineCreateWithAttributedString(attributed.as_CFTypeRef()) })?;
    let raw = unsafe { CTLineGetGlyphRuns(line.as_CFTypeRef()) };
    if raw.is_null() {
        return Err("无法排版文字".into());
    }
    let runs = unsafe { CFArray::<CFType>::wrap_under_get_rule(raw) };
    for run in runs.iter() {
        let n = unsafe { CTRunGetGlyphCount(run.as_CFTypeRef()) };
        if !(0..=1024).contains(&n) {
            return Err("文字排版超过限制".into());
        }
        let mut glyphs = vec![0; n as usize];
        unsafe { CTRunGetGlyphs(run.as_CFTypeRef(), CFRange::init(0, n), glyphs.as_mut_ptr()) };
        if glyphs.contains(&0) {
            return Err("系统字体缺少所需字符，请调整城市文字或字体".into());
        }
    }
    Ok(line)
}
fn jpeg(image: &CGImage, quality: u8) -> Result<Vec<u8>> {
    let raw = unsafe { CFDataCreateMutable(ptr::null(), 0) };
    if raw.is_null() {
        return Err("无法分配JPEG输出".into());
    }
    let data = unsafe { CFData::wrap_under_create_rule(raw as CFDataRef) };
    let kind = CFString::new("public.jpeg");
    let dest = owned(unsafe {
        CGImageDestinationCreateWithData(raw, kind.as_concrete_TypeRef(), 1, ptr::null())
    })?;
    let props = CFDictionary::from_CFType_pairs(&[(
        key(unsafe { kCGImageDestinationLossyCompressionQuality }),
        CFNumber::from(f64::from(quality) / 100.).as_CFType(),
    )]);
    unsafe {
        CGImageDestinationAddImage(
            dest.as_CFTypeRef(),
            image.as_ptr(),
            props.as_concrete_TypeRef(),
        );
    }
    if !unsafe { CGImageDestinationFinalize(dest.as_CFTypeRef()) } {
        return Err("JPEG编码失败".into());
    }
    if data.len() as u64 > MAX_INPUT_BYTES {
        return Err("水印JPEG超过32MiB输出上限".into());
    }
    Ok(data.bytes().to_vec())
}
pub fn render(bytes: &[u8], options: &Options, preview: Option<u32>) -> Result<Rendered> {
    render_image(bytes, options, preview, None)
}

pub fn render_planned(bytes: &[u8], options: &Options, info: &Info) -> Result<Rendered> {
    render_image(bytes, options, None, Some(info))
}

fn render_image(
    bytes: &[u8],
    options: &Options,
    preview: Option<u32>,
    planned: Option<&Info>,
) -> Result<Rendered> {
    let o = options.normalized()?;
    let (_data, source, props) = source(bytes)?;
    let info = metadata(&source, &props, &o, planned)?;
    if preview.is_none() && info.date.is_none() {
        return Err(info
            .warning
            .clone()
            .unwrap_or_else(|| "缺少拍摄日期".into()));
    }
    if preview.is_some_and(|n| !(320..=1600).contains(&n)) {
        return Err("水印预览尺寸无效".into());
    }
    let (export_width, export_height) =
        super::output_dimensions(info.width, info.height, o.output_max_edge);
    let max = preview
        .unwrap_or(export_width.max(export_height))
        .min(export_width.max(export_height));
    let manual = crate::imaging::unequal_density(&props);
    let orientation = number(&props, "Orientation").unwrap_or(1);
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
            CFNumber::from(i64::from(max)).as_CFType(),
        ),
    ];
    attrs.extend(crate::imaging::sdr_options(&source, &props)?);
    let attrs = CFDictionary::from_CFType_pairs(&attrs);
    let raw = unsafe {
        CGImageSourceCreateThumbnailAtIndex(source.as_CFTypeRef(), 0, attrs.as_concrete_TypeRef())
    };
    if raw.is_null() {
        return Err("系统无法解码这张照片".into());
    }
    let image =
        crate::imaging::display_thumbnail(unsafe { CGImage::from_ptr(raw) }, &source, &props)?;
    let (decoded_width, decoded_height) = if manual && orientation >= 5 {
        (image.height(), image.width())
    } else {
        (image.width(), image.height())
    };
    let (width, height) = super::output_dimensions(info.width, info.height, Some(max));
    // ImageIO may round a thumbnail edge differently by one pixel. The output
    // canvas uses our exact planned dimensions; larger mismatches remain errors.
    if decoded_width.abs_diff(width as usize) > 1 || decoded_height.abs_diff(height as usize) > 1 {
        return Err("照片方向或解码尺寸不一致，未输出".into());
    }
    let (width, height) = (width as usize, height as usize);
    let space =
        CGColorSpace::create_with_name(unsafe { kCGColorSpaceSRGB }).ok_or("sRGB色彩空间不可用")?;
    // Dimensions were checked before decode. The bitmap is 8-bit RGBA, at most 24MP.
    let raw_context = unsafe {
        CGBitmapContextCreate(
            ptr::null_mut(),
            width,
            height,
            8,
            width * 4,
            space.as_ptr(),
            kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
        )
    };
    if raw_context.is_null() {
        return Err("无法分配水印画布，请减小照片或稍后重试".into());
    }
    let context = unsafe { CGContext::from_ptr(raw_context) };
    context.scale(
        width as f64 / info.width as f64,
        height as f64 / info.height as f64,
    );
    let canvas = CGRect::new(
        &CGPoint::new(0., 0.),
        &CGSize::new(info.width as f64, info.height as f64),
    );
    context.set_rgb_fill_color(1., 1., 1., 1.);
    context.fill_rect(canvas);
    if manual {
        crate::imaging::draw_oriented(&context, canvas, &image, orientation);
    } else {
        context.draw_image(canvas, &image);
    }
    let mut bounds = None;
    let mut font_size = 0.;
    if !info.text.is_empty() {
        let short = info.width.min(info.height) as f64;
        let margin = short * o.margin_percent / 100.;
        font_size = (short * o.size_percent / 100.).max(4.);
        let mut text = line(&info.text, font_size, &o.font)?;
        let mut ink = unsafe { CTLineGetImageBounds(text.as_CFTypeRef(), ptr::null_mut()) };
        if ![ink.origin.x, ink.origin.y, ink.size.width, ink.size.height]
            .iter()
            .all(|n| n.is_finite())
            || ink.size.width <= 0.
            || ink.size.height <= 0.
        {
            return Err("水印排版无效".into());
        }
        let fit = ((info.width as f64 - 2. * margin) / ink.size.width)
            .min((info.height as f64 - 2. * margin) / ink.size.height)
            .min(1.);
        if fit < 1. {
            font_size *= fit;
            if font_size < 3. {
                return Err("水印文字过长或照片过小，请缩短城市名".into());
            }
            text = line(&info.text, font_size, &o.font)?;
            ink = unsafe { CTLineGetImageBounds(text.as_CFTypeRef(), ptr::null_mut()) };
        }
        let x = info.width as f64 - margin - ink.size.width;
        let y = margin;
        if x < 0. || y + ink.size.height > info.height as f64 {
            return Err("水印超出照片范围".into());
        }
        let c = u32::from_str_radix(&o.color[1..], 16).map_err(|_| "颜色无效")?;
        context.set_rgb_fill_color(
            ((c >> 16) & 255) as f64 / 255.,
            ((c >> 8) & 255) as f64 / 255.,
            (c & 255) as f64 / 255.,
            o.opacity,
        );
        if o.shadow {
            context.set_shadow(CGSize::new(0., -font_size * 0.025), font_size * 0.08);
        }
        context.set_text_position(x - ink.origin.x, y - ink.origin.y);
        unsafe {
            CTLineDraw(text.as_CFTypeRef(), context.as_ptr());
        }
        bounds = Some(Bounds {
            x,
            y: info.height as f64 - y - ink.size.height,
            width: ink.size.width,
            height: ink.size.height,
        });
    }
    let output = context.create_image().ok_or("无法生成水印画面")?;
    Ok(Rendered {
        jpeg: jpeg(&output, o.jpeg_quality)?,
        info: RenderInfo {
            source: info,
            output_width: width as u32,
            output_height: height as u32,
            bounds,
            font_size,
        },
    })
}
