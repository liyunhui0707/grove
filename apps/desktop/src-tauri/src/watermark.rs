//! Date/city derivatives, separate from originals and portable-copy exports.
use crate::catalog::Result;
use serde::{Deserialize, Serialize};
use std::io::Read;

pub const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 24_000_000;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub city: String,
    pub fallback_date: Option<String>,
    pub font: String,
    pub size_percent: f64,
    pub color: String,
    pub opacity: f64,
    pub margin_percent: f64,
    pub shadow: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            city: String::new(),
            fallback_date: None,
            font: "sans".into(),
            size_percent: 2.0,
            color: "#FFFFFF".into(),
            opacity: 0.88,
            margin_percent: 2.5,
            shadow: true,
        }
    }
}
impl Options {
    pub fn normalized(&self) -> Result<Self> {
        let mut o = self.clone();
        o.city = o.city.trim().to_string();
        if o.city.chars().count() > 32
            || o.city.chars().any(|c| {
                c.is_control() || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
            })
        {
            return Err("城市限32个字符，不含换行或控制字符".into());
        }
        if !["sans", "serif"].contains(&o.font.as_str())
            || !o.size_percent.is_finite()
            || !(0.8..=5.0).contains(&o.size_percent)
            || !o.opacity.is_finite()
            || !(0.15..=1.0).contains(&o.opacity)
            || !o.margin_percent.is_finite()
            || !(0.5..=8.0).contains(&o.margin_percent)
        {
            return Err("水印字体、字号、透明度或边距无效".into());
        }
        if o.color.len() != 7
            || !o.color.starts_with('#')
            || !o.color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        {
            return Err("水印颜色无效".into());
        }
        o.fallback_date = o.fallback_date.filter(|s| !s.is_empty());
        if o.fallback_date.as_deref().is_some_and(|s| !valid_date(s)) {
            return Err("备用日期必须为有效的YYYY-MM-DD，不会自动使用文件时间".into());
        }
        Ok(o)
    }
}
pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return false;
    }
    let year = s[..4].parse::<u32>().unwrap_or(0);
    let month = s[5..7].parse::<u32>().unwrap_or(0);
    let day = s[8..].parse::<u32>().unwrap_or(0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    (1..=9999).contains(&year) && day > 0 && day <= days
}
pub fn exif_date(s: &str) -> Option<String> {
    let s = s.trim_matches('\0').trim();
    let b = s.as_bytes();
    if b.len() != 19
        || b[4] != b':'
        || b[7] != b':'
        || b[10] != b' '
        || b[13] != b':'
        || b[16] != b':'
        || !b
            .iter()
            .enumerate()
            .all(|(i, c)| [4, 7, 10, 13, 16].contains(&i) || c.is_ascii_digit())
    {
        return None;
    }
    let date = s[..10].replace(':', "-");
    if !valid_date(&date)
        || s[11..13].parse::<u8>().ok()? > 23
        || s[14..16].parse::<u8>().ok()? > 59
        || s[17..19].parse::<u8>().ok()? > 59
    {
        return None;
    }
    Some(date)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub date: Option<String>,
    pub date_source: String,
    pub text: String,
    pub source_profile: Option<String>,
    pub warning: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderInfo {
    pub source: Info,
    pub output_width: u32,
    pub output_height: u32,
    pub bounds: Option<Bounds>,
    pub font_size: f64,
}
pub struct Rendered {
    pub jpeg: Vec<u8>,
    pub info: RenderInfo,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub data_url: String,
    pub info: RenderInfo,
    pub can_export: bool,
}
pub fn read_input(file: std::fs::File) -> Result<Vec<u8>> {
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 || size > MAX_INPUT_BYTES {
        return Err("首版水印限单张32MiB以内".into());
    }
    // Reserve once from the already-open file. Repeated geometric growth of
    // Take<File>'s generic read buffer can retain hundreds of MiB in a batch.
    // The extra byte detects growth without allocating another large buffer.
    let mut bytes = Vec::with_capacity(size as usize + 1);
    file.take(size + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size {
        return Err("照片在读取时发生变化，请重新核对".into());
    }
    Ok(bytes)
}
pub fn caption(
    width: u32,
    height: u32,
    date: Option<String>,
    profile: Option<String>,
    o: &Options,
) -> Result<Info> {
    if width == 0
        || height == 0
        || width.max(height) > 12000
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err("首版水印限2400万像素、最长边12000px".into());
    }
    let (date, date_source) = match date {
        Some(d) => (Some(d), "exif"),
        None => (
            o.fallback_date.clone(),
            if o.fallback_date.is_some() {
                "manual"
            } else {
                "missing"
            },
        ),
    };
    let text = date
        .as_ref()
        .map(|d| {
            if o.city.is_empty() {
                d.clone()
            } else {
                format!("{d} {}", o.city)
            }
        })
        .unwrap_or_default();
    Ok(Info {
        width,
        height,
        date,
        date_source: date_source.into(),
        text,
        source_profile: profile,
        warning: if date_source == "missing" {
            Some("未找到有效EXIF拍摄日期，请填写备用日期；不会使用文件修改时间".into())
        } else {
            None
        },
    })
}
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{inspect, render};
#[cfg(not(target_os = "macos"))]
pub fn inspect(_: &[u8], _: &Options) -> Result<Info> {
    Err("此平台水印渲染尚未验证".into())
}
#[cfg(not(target_os = "macos"))]
pub fn render(_: &[u8], _: &Options, _: Option<u32>) -> Result<Rendered> {
    Err("此平台水印渲染尚未验证".into())
}

#[cfg(test)]
pub(crate) mod tests;
