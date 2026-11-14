use crate::catalog::Result;
use image::{Rgb, RgbImage};
use std::{fs, path::Path};

// Original procedural landscapes, not downloaded photographs. Files are generated outside Git.
pub fn generate(root: &Path) -> Result<()> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    for index in 0..24 {
        let ext = match index {
            20 => "png",
            21 => "webp",
            _ => "jpg",
        };
        let file = root.join(format!("山海_{:02}.{ext}", index + 1));
        if file.exists() {
            continue;
        }
        let (w, h) = if index % 5 == 0 {
            (960, 1280)
        } else {
            (1440, 960)
        };
        let palettes = [
            ([178., 199., 196.], [232., 216., 182.], [46., 83., 79.]),
            ([165., 183., 205.], [231., 215., 202.], [58., 78., 106.]),
            ([220., 191., 158.], [243., 224., 187.], [111., 114., 77.]),
            ([187., 191., 180.], [231., 221., 199.], [59., 86., 77.]),
        ];
        let (sky, sand, dark) = palettes[index % 4];
        let phase = index as f32 * 0.61;
        let img = RgbImage::from_fn(w, h, |x, y| {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let horizon = 0.48 + 0.06 * (phase + fx * 4.).sin();
            let hill1 = horizon - 0.12 * (fx * 8. + phase).sin() - 0.06 * (fx * 19. + phase).cos();
            let hill2 = 0.64 + 0.08 * (fx * 9. + phase + 1.).sin();
            let hill3 = 0.85 + 0.09 * (fx * 5. + phase).sin();
            let grain = ((x * 1973 + y * 9277 + index as u32 * 26699) % 127) as f32 / 127. - 0.5;
            let sun = ((fx - 0.72).powi(2) + (fy - 0.23).powi(2)).sqrt();
            let mut c = sky.map(|v| v + (1. - fy) * 17.);
            if sun < 0.053 {
                c = [246., 232., 199.];
            }
            if fy > hill1 {
                c = std::array::from_fn(|i| sky[i] * 0.40 + dark[i] * 0.60 + 22.);
            }
            if fy > hill2 {
                c = if index % 3 == 0 {
                    std::array::from_fn(|i| {
                        sky[i] * 0.7 + dark[i] * 0.3 + (fx * 120. + fy * 210.).sin() * 3.
                    })
                } else {
                    std::array::from_fn(|i| dark[i] * 0.9 + 14.)
                };
            }
            if fy > hill3 {
                c = if index % 3 == 0 { sand } else { dark };
            }
            Rgb(c.map(|v| (v + grain * 3.).clamp(0., 255.) as u8))
        });
        img.save(&file).map_err(|e| e.to_string())?;
    }
    Ok(())
}
