//! Reproducible synthetic corpus. Run the same release binary three times per case.
//! cargo run -p squeeze-pic-core --release --example compression_bench -- LABEL [DIR]
use image::{ImageEncoder, Rgba, RgbaImage};
use sha2::{Digest, Sha256};
use squeeze_pic_core::*;
use std::{fs, path::PathBuf, sync::atomic::AtomicBool, time::Instant};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let label = args.next().unwrap_or_else(|| "current".into());
    let root = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "tmp/compression-benchmark".into()),
    );
    fs::create_dir_all(&root)?;
    let palette = [
        [32, 35, 42],
        [245, 245, 248],
        [85, 103, 195],
        [148, 150, 162],
        [226, 227, 232],
        [184, 82, 65],
        [237, 197, 86],
        [82, 132, 154],
    ];
    let ui = RgbaImage::from_fn(768, 512, |x, y| {
        let n = ((x / 19 + y / 31) % palette.len() as u32) as usize;
        let p = palette[n];
        Rgba([p[0], p[1], p[2], 255])
    });
    let gray = RgbaImage::from_fn(768, 512, |x, y| {
        let v = (((x / 7 + y / 11) % 16) * 17) as u8;
        Rgba([v, v, v, 255])
    });
    let rgb = RgbaImage::from_fn(768, 512, |x, y| {
        let noise = ((x.wrapping_mul(1664525) ^ y.wrapping_mul(1013904223)) >> 17) as u8 % 13;
        Rgba([
            (x * 211 / 768) as u8 + noise,
            (y * 211 / 512) as u8 + noise,
            ((x + y) * 211 / 1280) as u8 + noise,
            255,
        ])
    });
    let alpha = RgbaImage::from_fn(768, 512, |x, y| {
        let p = palette[((x / 13 + y / 17) % 8) as usize];
        Rgba([
            p[0],
            p[1],
            p[2],
            [0, 85, 170, 255][((x / 23 + y / 29) % 4) as usize],
        ])
    });
    for (name, pixels) in [
        ("ui", &ui),
        ("gray", &gray),
        ("rgb", &rgb),
        ("alpha", &alpha),
    ] {
        let path = root.join(format!("{name}.png"));
        if !path.exists() {
            pixels.save(path)?;
        }
    }
    let icc_path = root.join("gray-icc.png");
    if !icc_path.exists() {
        let mut file = fs::File::create(&icc_path)?;
        let mut encoder = image::codecs::png::PngEncoder::new(&mut file);
        encoder.set_icc_profile(moxcms::ColorProfile::new_srgb().encode()?)?;
        encoder.write_image(
            &gray,
            gray.width(),
            gray.height(),
            image::ExtendedColorType::Rgba8,
        )?;
    }
    let mut cases = Vec::new();
    for name in ["ui", "gray", "rgb", "alpha", "gray-icc"] {
        cases.push((
            format!("png-{name}"),
            name,
            ProcessOptions {
                preset: Preset::Lossless,
                ..Default::default()
            },
        ));
    }
    for name in ["rgb", "alpha"] {
        cases.push((
            format!("jpeg-{name}"),
            name,
            ProcessOptions {
                format: Some(Format::Jpeg),
                ..Default::default()
            },
        ));
    }
    cases.push((
        "jpeg-resize".into(),
        "rgb",
        ProcessOptions {
            format: Some(Format::Jpeg),
            resize: Resize {
                mode: ResizeMode::Width,
                value: 384,
                ..Default::default()
            },
            ..Default::default()
        },
    ));
    cases.push((
        "webp-deep".into(),
        "alpha",
        ProcessOptions {
            format: Some(Format::Webp),
            preset: Preset::Lossless,
            strategy: Strategy::Deep,
            ..Default::default()
        },
    ));
    let mut rows = Vec::new();
    for (case, name, options) in cases {
        let input = root.join(format!("{name}.png"));
        let original = fs::read(&input)?;
        let mut timings = Vec::new();
        let mut last = None;
        let mut digest = String::new();
        let mut ihdr = None;
        for _ in 0..3 {
            let dir = tempfile::tempdir_in(&root)?;
            let format = options.format.unwrap_or(Format::Png);
            let output = dir.path().join(format!("out.{}", format.extension()));
            let started = Instant::now();
            let report = process_file(&input, &output, &options, &AtomicBool::new(false), |_| {})?;
            timings.push(started.elapsed().as_secs_f64() * 1000.);
            let bytes = fs::read(report.output.as_deref().unwrap_or(&input))?;
            digest = format!("{:x}", Sha256::digest(&bytes));
            if format == Format::Png {
                ihdr = Some([bytes[24], bytes[25]]);
                assert_eq!(
                    image::load_from_memory(&bytes)?.into_rgba8(),
                    image::load_from_memory(&original)?.into_rgba8()
                );
            }
            last = Some(report);
        }
        timings.sort_by(f64::total_cmp);
        let report = last.unwrap();
        assert_eq!(fs::read(&input)?, original);
        rows.push(
            serde_json::json!({"case": case, "bytes": report.result_bytes,
            "median_ms": timings[1], "samples_ms": timings, "sha256": digest,
            "encoding": report.encoding, "png_depth_color": ihdr}),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({"label": label, "rows": rows}))?
    );
    Ok(())
}
