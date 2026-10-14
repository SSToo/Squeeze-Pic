//! Generate fixtures for the second optimization pass; no user images are read.
use image::{Rgba, RgbaImage};
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new("tmp/deep-benchmark");
    fs::create_dir_all(dir)?;
    for name in ["ui", "gray", "rgb", "alpha", "gray-icc"] {
        let input = Path::new("tmp/compression-benchmark").join(format!("{name}.png"));
        if input.exists() {
            image::open(input)?
                .into_rgba8()
                .save(dir.join(format!("{name}.bmp")))?;
        }
    }
    let pixels = RgbaImage::from_fn(2048, 2048, |x, y| {
        Rgba([
            (x / 8 % 256) as u8,
            (y / 8 % 256) as u8,
            ((x + y) / 16 % 256) as u8,
            255,
        ])
    });
    pixels.save(dir.join("large.bmp"))?;
    println!("Fixtures written to {}", dir.display());
    Ok(())
}
