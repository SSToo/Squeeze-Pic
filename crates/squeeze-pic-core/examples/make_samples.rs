use image::{ImageEncoder, Rgba, RgbaImage};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "tmp/samples".into());
    fs::create_dir_all(&dir)?;
    let pixels = RgbaImage::from_fn(1200, 800, |x, y| {
        let sky = [
            180. + y as f32 * 0.04,
            217. - y as f32 * 0.025,
            214. - y as f32 * 0.02,
        ];
        let ridge = 450. + (x as f32 / 170.).sin() * 100.;
        let front = 640. + (x as f32 / 240. + 1.).sin() * 70.;
        let color = if y as f32 > front {
            [36., 99., 76.]
        } else if y as f32 > ridge {
            [82., 141., 113.]
        } else if (x as i32 - 925).pow(2) + (y as i32 - 180).pow(2) < 65 * 65 {
            [250., 235., 194.]
        } else {
            sky
        };
        Rgba([color[0] as u8, color[1] as u8, color[2] as u8, 255])
    });
    pixels.save(dir.join("landscape.png"))?;
    let transparent = RgbaImage::from_fn(600, 600, |x, y| {
        let d = ((x as f32 - 300.).powi(2) + (y as f32 - 300.).powi(2)).sqrt();
        let alpha = ((240. - d).clamp(0., 40.) / 40. * 255.).round() as u8;
        Rgba([33, 130, 93, alpha])
    });
    transparent.save(dir.join("transparent.png"))?;
    let mut rotated = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut rotated);
    encoder.set_exif_metadata(vec![
        b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
    ])?;
    encoder.write_image(&pixels, 1200, 800, image::ExtendedColorType::Rgba8)?;
    fs::write(dir.join("orientation-6.png"), rotated)?;
    println!("Created sample images in {}", dir.display());
    Ok(())
}
