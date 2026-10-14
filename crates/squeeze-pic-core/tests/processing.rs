use image::{ImageEncoder, Rgba, RgbaImage};
use squeeze_pic_core::*;
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
};

fn sample(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x * 3) as u8,
            (y * 7) as u8,
            (x + y) as u8,
            if x < width / 3 {
                0
            } else {
                (x % 127 + 128) as u8
            },
        ])
    })
}
fn process(
    input: &std::path::Path,
    output: &std::path::Path,
    options: &ProcessOptions,
) -> Result<ProcessReport> {
    process_file(input, output, options, &AtomicBool::new(false), |_| {})
}

#[test]
fn png_reductions_keep_exact_pixels_including_hidden_rgb() {
    let dir = tempfile::tempdir().unwrap();
    let colors = [[11, 73, 154], [213, 25, 84], [94, 183, 27], [241, 182, 64]];
    for transparent in [false, true] {
        let pixels = RgbaImage::from_fn(192, 128, |x, y| {
            let n = ((x.wrapping_mul(1664525) ^ y.wrapping_mul(1013904223)) >> 12) as usize;
            let c = colors[n % colors.len()];
            Rgba([
                c[0],
                c[1],
                c[2],
                if transparent {
                    [0, 85, 170, 255][n / 4 % 4]
                } else {
                    255
                },
            ])
        });
        let input = dir.path().join(format!("palette-{transparent}.png"));
        let output = dir.path().join(format!("compact-{transparent}.png"));
        pixels.save(&input).unwrap();
        let original = fs::read(&input).unwrap();
        process(
            &input,
            &output,
            &ProcessOptions {
                preset: Preset::Lossless,
                ..Default::default()
            },
        )
        .unwrap();
        let encoded = fs::read(&output).unwrap();
        assert_eq!(
            encoded[25], 3,
            "Use indexed PNG when exact colors fit a palette"
        );
        assert_eq!(image::open(&output).unwrap().into_rgba8(), pixels);
        assert!(encoded.len() < original.len());
        assert_eq!(fs::read(input).unwrap(), original);
    }
}

#[test]
fn png_grayscale_reduces_storage_depth_without_quantization() {
    let dir = tempfile::tempdir().unwrap();
    let pixels = RgbaImage::from_fn(192, 128, |x, y| {
        let v = (((x * 13 + y * 7) % 16) * 17) as u8;
        Rgba([v, v, v, 255])
    });
    let input = dir.path().join("rgba.png");
    pixels.save(&input).unwrap();
    for strategy in [Strategy::Fast, Strategy::Smart, Strategy::Deep] {
        let output = dir.path().join(format!("gray-{strategy:?}.png"));
        process(
            &input,
            &output,
            &ProcessOptions {
                preset: Preset::Lossless,
                strategy,
                ..Default::default()
            },
        )
        .unwrap();
        let bytes = fs::read(&output).unwrap();
        assert_eq!(
            [bytes[24], bytes[25]],
            [4, 0],
            "16 exact gray values need only four storage bits"
        );
        assert_eq!(image::open(output).unwrap().into_rgba8(), pixels);
    }
}

#[test]
fn png_grayscale_pixels_with_rgb_icc_keep_a_compatible_color_type() {
    use image::ImageDecoder;
    let dir = tempfile::tempdir().unwrap();
    let pixels = RgbaImage::from_fn(192, 128, |x, y| {
        let v = (((x * 13 + y * 7) % 16) * 17) as u8;
        Rgba([v, v, v, 255])
    });
    let profile = moxcms::ColorProfile::new_srgb().encode().unwrap();
    let input = dir.path().join("rgb-profile.png");
    let output = dir.path().join("compact.png");
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder.set_icc_profile(profile.clone()).unwrap();
    encoder
        .write_image(
            &pixels,
            pixels.width(),
            pixels.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    fs::write(&input, bytes).unwrap();
    process(
        &input,
        &output,
        &ProcessOptions {
            preset: Preset::Lossless,
            ..Default::default()
        },
    )
    .unwrap();
    let encoded = fs::read(&output).unwrap();
    assert!(
        matches!(encoded[25], 2 | 3 | 6),
        "An RGB ICC must not be attached to a grayscale PNG"
    );
    let mut decoder = image::ImageReader::open(&output)
        .unwrap()
        .with_guessed_format()
        .unwrap()
        .into_decoder()
        .unwrap();
    assert_eq!(decoder.icc_profile().unwrap().unwrap(), profile);
    assert_eq!(image::open(output).unwrap().into_rgba8(), pixels);
}

#[test]
fn png_webp_lossless_roundtrip_retains_invisible_rgb_and_original() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.png");
    let webp = dir.path().join("out.webp");
    let png = dir.path().join("back.png");
    let pixels = sample(96, 80);
    pixels.save(&input).unwrap();
    let original = fs::read(&input).unwrap();
    let options = ProcessOptions {
        format: Some(Format::Webp),
        preset: Preset::Lossless,
        ..Default::default()
    };
    assert_eq!(process(&input, &webp, &options).unwrap().outcome, "saved");
    assert_eq!(image::open(&webp).unwrap().into_rgba8(), pixels);
    let options = ProcessOptions {
        format: Some(Format::Png),
        preset: Preset::Lossless,
        ..Default::default()
    };
    process(&webp, &png, &options).unwrap();
    assert_eq!(image::open(png).unwrap().into_rgba8(), pixels);
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn tiff_strategies_compress_losslessly_and_tag_straight_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.png");
    let pixels = sample(192, 128);
    pixels.save(&input).unwrap();
    for strategy in [Strategy::Fast, Strategy::Smart, Strategy::Deep] {
        let output = dir.path().join(format!("{strategy:?}.tiff"));
        let report = process(
            &input,
            &output,
            &ProcessOptions {
                format: Some(Format::Tiff),
                preset: Preset::Lossless,
                strategy,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(report.result_bytes < pixels.len() as u64);
        assert_eq!(image::open(&output).unwrap().into_rgba8(), pixels);
        let mut decoder = tiff::decoder::Decoder::new(fs::File::open(output).unwrap()).unwrap();
        assert_eq!(
            decoder
                .get_tag_unsigned::<u16>(tiff::tags::Tag::Compression)
                .unwrap(),
            8
        );
        assert_eq!(
            decoder
                .get_tag_u16_vec(tiff::tags::Tag::ExtraSamples)
                .unwrap(),
            [2]
        );
    }
}

#[test]
fn bmp_direct_encoder_keeps_alpha_and_hidden_colors() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.png");
    let output = dir.path().join("out.bmp");
    let pixels = sample(96, 64);
    pixels.save(&input).unwrap();
    process(
        &input,
        &output,
        &ProcessOptions {
            format: Some(Format::Bmp),
            preset: Preset::Lossless,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(image::open(output).unwrap().into_rgba8(), pixels);
}
#[test]
fn all_static_formats_decode_and_convert_with_expected_dimensions() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.png");
    sample(120, 80).save(&input).unwrap();
    for format in [Format::Jpeg, Format::Png, Format::Webp] {
        let output = dir.path().join(format!("out.{}", format.extension()));
        let options = ProcessOptions {
            format: Some(format),
            resize: Resize {
                mode: ResizeMode::Width,
                value: 60,
                ..Default::default()
            },
            ..Default::default()
        };
        let report = process(&input, &output, &options).unwrap();
        assert_eq!(report.after_dimensions, [60, 40]);
        assert_eq!(
            [
                inspect_file(&output).unwrap().width,
                inspect_file(&output).unwrap().height
            ],
            [60, 40]
        );
        let target = dir.path().join(format!("from-{}.png", format.extension()));
        let options = ProcessOptions {
            format: Some(Format::Png),
            resize: Resize {
                mode: ResizeMode::Width,
                value: 30,
                ..Default::default()
            },
            ..Default::default()
        };
        process(&output, &target, &options).unwrap();
    }
}
#[test]
fn existing_output_is_never_overwritten_and_cancel_does_not_commit() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.png");
    let output = dir.path().join("out.jpg");
    sample(32, 24).save(&input).unwrap();
    fs::write(&output, b"keep this").unwrap();
    let options = ProcessOptions {
        format: Some(Format::Jpeg),
        ..Default::default()
    };
    assert_eq!(
        process(&input, &output, &options).unwrap_err().code,
        "exists"
    );
    assert_eq!(fs::read(&output).unwrap(), b"keep this");
    fs::remove_file(&output).unwrap();
    assert_eq!(
        process_file(&input, &output, &options, &AtomicBool::new(true), |_| {})
            .unwrap_err()
            .code,
        "cancelled"
    );
    assert!(!output.exists());
    let cancelled = AtomicBool::new(false);
    let error = process_file(&input, &output, &options, &cancelled, |phase| {
        if phase == "saving" {
            cancelled.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "cancelled");
    assert!(!output.exists());
}
#[test]
fn input_changes_and_output_races_are_caught_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.png");
    let output = dir.path().join("out.jpg");
    sample(32, 24).save(&input).unwrap();
    let options = ProcessOptions {
        format: Some(Format::Jpeg),
        ..Default::default()
    };
    let error = process_file(
        &input,
        &output,
        &options,
        &AtomicBool::new(false),
        |phase| {
            if phase == "saving" {
                fs::write(&input, b"changed externally").unwrap();
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "input_changed");
    assert!(!output.exists());
    sample(32, 24).save(&input).unwrap();
    assert_eq!(
        process_file(
            &input,
            &output,
            &options,
            &AtomicBool::new(false),
            |phase| {
                if phase == "saving" {
                    fs::write(&output, b"other task").unwrap();
                }
            }
        )
        .unwrap_err()
        .code,
        "commit"
    );
    assert_eq!(fs::read(output).unwrap(), b"other task");
}
#[test]
fn rejects_lossless_resize_and_lossless_jpeg_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.png");
    sample(32, 24).save(&input).unwrap();
    let mut options = ProcessOptions {
        preset: Preset::Lossless,
        ..Default::default()
    };
    options.resize.mode = ResizeMode::Width;
    assert_eq!(
        process(&input, &dir.path().join("out.png"), &options)
            .unwrap_err()
            .code,
        "lossless_resize"
    );
    options.resize.mode = ResizeMode::Original;
    options.format = Some(Format::Jpeg);
    assert_eq!(
        process(&input, &dir.path().join("out.jpg"), &options)
            .unwrap_err()
            .code,
        "lossless_jpeg"
    );
}
#[test]
fn dimensions_preserve_aspect_ratio_and_disable_accidental_upscale() {
    for (mode, value, expected) in [
        (ResizeMode::Percent, 50, [600, 400]),
        (ResizeMode::Longest, 600, [600, 400]),
        (ResizeMode::Width, 300, [300, 200]),
        (ResizeMode::Height, 300, [450, 300]),
    ] {
        assert_eq!(
            output_dimensions(
                1200,
                800,
                &Resize {
                    mode,
                    value,
                    ..Default::default()
                }
            )
            .unwrap(),
            expected
        );
    }
    let mut resize = Resize {
        mode: ResizeMode::Fit,
        width: 500,
        height: 200,
        ..Default::default()
    };
    assert_eq!(output_dimensions(1200, 800, &resize).unwrap(), [300, 200]);
    resize.mode = ResizeMode::Width;
    resize.value = 2400;
    assert_eq!(output_dimensions(1200, 800, &resize).unwrap(), [1200, 800]);
    resize.allow_upscale = true;
    assert_eq!(output_dimensions(1200, 800, &resize).unwrap(), [2400, 1600]);
    resize.value = 0;
    assert!(output_dimensions(1200, 800, &resize).is_err());
}
fn exif_orientation_6() -> Vec<u8> {
    vec![
        b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
    ]
}
#[test]
fn orientation_is_applied_before_resize_and_normalized_in_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("rotated.png");
    let output = dir.path().join("rotated.webp");
    let pixels = sample(80, 40);
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder.set_exif_metadata(exif_orientation_6()).unwrap();
    encoder
        .write_image(&pixels, 80, 40, image::ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(&input, bytes).unwrap();
    let info = inspect_file(&input).unwrap();
    assert_eq!([info.display_width, info.display_height], [40, 80]);
    assert_eq!(info.orientation, 6);
    let options = ProcessOptions {
        format: Some(Format::Webp),
        resize: Resize {
            mode: ResizeMode::Width,
            value: 20,
            ..Default::default()
        },
        ..Default::default()
    };
    process(&input, &output, &options).unwrap();
    let info = inspect_file(output).unwrap();
    assert_eq!([info.width, info.height], [20, 40]);
    assert_eq!(info.orientation, 1);
    assert!(info.has_exif);
}
#[test]
fn icc_is_preserved_in_strict_conversion_and_srgb_after_pixel_processing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("p3.png");
    let output = dir.path().join("p3.webp");
    let pixels = sample(64, 48);
    let icc = moxcms::ColorProfile::new_display_p3().encode().unwrap();
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder.set_icc_profile(icc.clone()).unwrap();
    encoder
        .write_image(&pixels, 64, 48, image::ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(&input, bytes).unwrap();
    process(
        &input,
        &output,
        &ProcessOptions {
            preset: Preset::Lossless,
            format: Some(Format::Webp),
            ..Default::default()
        },
    )
    .unwrap();
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(&output)
        .unwrap()
        .with_guessed_format()
        .unwrap()
        .into_decoder()
        .unwrap();
    assert_eq!(decoder.icc_profile().unwrap().unwrap(), icc);
    drop(decoder);
    let output = dir.path().join("srgb.jpg");
    process(
        &input,
        &output,
        &ProcessOptions {
            format: Some(Format::Jpeg),
            ..Default::default()
        },
    )
    .unwrap();
    let mut decoder = image::ImageReader::open(output)
        .unwrap()
        .with_guessed_format()
        .unwrap()
        .into_decoder()
        .unwrap();
    assert_eq!(
        decoder.icc_profile().unwrap().unwrap(),
        moxcms::ColorProfile::new_srgb().encode().unwrap()
    );
}
#[test]
fn transparent_resampling_does_not_bleed_hidden_black_into_red_edges() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("alpha.png");
    let output = dir.path().join("small.png");
    let pixels = RgbaImage::from_fn(40, 40, |x, _| {
        if x < 20 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([255, 0, 0, 255])
        }
    });
    pixels.save(&input).unwrap();
    process(
        &input,
        &output,
        &ProcessOptions {
            resize: Resize {
                mode: ResizeMode::Percent,
                value: 50,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    for p in image::open(output).unwrap().into_rgba8().pixels() {
        if p[3] > 0 && p[3] < 255 {
            assert!(p[0] >= 250);
            assert_eq!(p[1], 0);
            assert_eq!(p[2], 0);
        }
    }
}
#[test]
fn rejects_16_bit_corruption_and_extension_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("16.png");
    image::ImageBuffer::<image::Luma<u16>, _>::from_pixel(4, 4, image::Luma([1000]))
        .save(&input)
        .unwrap();
    assert_eq!(inspect_file(&input).unwrap_err().code, "bit_depth");
    sample(16, 16).save(&input).unwrap();
    assert_eq!(
        process(
            &input,
            &dir.path().join("wrong.jpg"),
            &ProcessOptions::default()
        )
        .unwrap_err()
        .code,
        "output_format"
    );
    let mut corrupt = fs::read(&input).unwrap();
    corrupt[45] ^= 1;
    fs::write(&input, corrupt).unwrap();
    assert_eq!(inspect_file(input).unwrap_err().code, "decode");
}
#[test]
fn preview_obeys_visible_orientation_and_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.png");
    let output = dir.path().join("preview.png");
    sample(320, 160).save(&input).unwrap();
    create_preview(&input, &output, 80).unwrap();
    assert_eq!(image::image_dimensions(output).unwrap(), (80, 40));
}

#[test]
fn animation_is_rejected_instead_of_silently_extracting_a_frame() {
    let dir = tempfile::tempdir().unwrap();
    let pixels = sample(32, 32);
    let other = RgbaImage::from_pixel(32, 32, Rgba([255, 0, 0, 255]));
    let config = webp::WebPConfig::new().unwrap();
    let mut encoder = webp::AnimEncoder::new(32, 32, &config);
    encoder.add_frame(webp::AnimFrame::from_rgba(&pixels, 32, 32, 0));
    encoder.add_frame(webp::AnimFrame::from_rgba(&other, 32, 32, 100));
    let webp = dir.path().join("animated.webp");
    fs::write(&webp, &*encoder.encode()).unwrap();
    assert_eq!(inspect_file(webp).unwrap_err().code, "animation");

    // Build a valid two-frame APNG using independently generated PNG IDAT data.
    let input = dir.path().join("static.png");
    pixels.save(&input).unwrap();
    let bytes = fs::read(input).unwrap();
    let mut output = bytes[..33].to_vec();
    fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(tag);
        out.extend(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(tag);
        crc.update(data);
        out.extend(crc.finalize().to_be_bytes());
    }
    chunk(&mut output, b"acTL", &[0, 0, 0, 2, 0, 0, 0, 0]);
    let mut frame = Vec::new();
    frame.extend(0u32.to_be_bytes());
    frame.extend(32u32.to_be_bytes());
    frame.extend(32u32.to_be_bytes());
    frame.extend([0; 8]);
    frame.extend([0, 1, 0, 10, 0, 0]);
    chunk(&mut output, b"fcTL", &frame);
    let mut idat: Vec<u8> = Vec::new();
    let mut offset = 33;
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        if &bytes[offset + 4..offset + 8] == b"IDAT" {
            let data = &bytes[offset + 8..offset + 8 + length];
            idat.extend(data);
            chunk(&mut output, b"IDAT", data);
        }
        offset += length + 12;
    }
    frame[..4].copy_from_slice(&1u32.to_be_bytes());
    chunk(&mut output, b"fcTL", &frame);
    let mut data = 2u32.to_be_bytes().to_vec();
    data.extend(idat);
    chunk(&mut output, b"fdAT", &data);
    chunk(&mut output, b"IEND", &[]);
    let animated = dir.path().join("animated.png");
    fs::write(&animated, output).unwrap();
    assert_eq!(inspect_file(animated).unwrap_err().code, "animation");
}

#[test]
fn truncated_jpeg_is_rejected_before_processing() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("in.png");
    let jpg = dir.path().join("out.jpg");
    sample(120, 80).save(&png).unwrap();
    process(
        &png,
        &jpg,
        &ProcessOptions {
            format: Some(Format::Jpeg),
            ..Default::default()
        },
    )
    .unwrap();
    let mut bytes = fs::read(&jpg).unwrap();
    bytes.truncate(bytes.len() - 16);
    fs::write(&jpg, bytes).unwrap();
    assert_eq!(inspect_file(jpg).unwrap_err().code, "decode");
}

#[test]
fn additional_formats_roundtrip_and_icons_keep_aspect_ratio() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.png");
    sample(120, 80).save(&input).unwrap();
    let mut formats = vec![
        Format::Gif,
        Format::Bmp,
        Format::Tiff,
        Format::Ico,
        Format::Icns,
        Format::Jxl,
    ];
    if cfg!(target_os = "macos") {
        formats.extend([Format::Avif, Format::Heif]);
    }
    for format in formats {
        let output = dir.path().join(format!("output.{}", format.extension()));
        let options = ProcessOptions {
            format: Some(format),
            resize: Resize {
                mode: ResizeMode::Width,
                value: 60,
                ..Default::default()
            },
            ..Default::default()
        };
        let report =
            process(&input, &output, &options).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        let expected = match format {
            Format::Ico => [60, 60],
            Format::Icns => [32, 32],
            _ => [60, 40],
        };
        assert_eq!(report.after_dimensions, expected, "{format:?}");
        let back = dir.path().join(format!("back-{}.png", format.extension()));
        let report = process(
            &output,
            &back,
            &ProcessOptions {
                format: Some(Format::Png),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("decode {format:?}: {e}"));
        assert_eq!(report.after_dimensions, expected);
        if matches!(format, Format::Ico | Format::Icns) {
            let pixels = image::open(&back).unwrap().into_rgba8();
            assert_eq!(
                pixels.get_pixel(expected[0] / 2, 0)[3],
                0,
                "icon must be padded, not stretched"
            );
        }
    }
    let mut family = icns::IconFamily::new();
    for edge in [16, 32] {
        family
            .add_icon(
                &icns::Image::from_data(
                    icns::PixelFormat::RGBA,
                    edge,
                    edge,
                    sample(edge, edge).into_raw(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let multi = dir.path().join("multi.icns");
    family.write(fs::File::create(&multi).unwrap()).unwrap();
    let error = process(
        &multi,
        &dir.path().join("compressed.icns"),
        &ProcessOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, "icon_family");
}
#[test]
fn jxl_strict_lossless_retains_hidden_rgb() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.png");
    let pixels = sample(64, 48);
    pixels.save(&input).unwrap();
    let jxl = dir.path().join("out.jxl");
    process(
        &input,
        &jxl,
        &ProcessOptions {
            format: Some(Format::Jxl),
            preset: Preset::Lossless,
            ..Default::default()
        },
    )
    .unwrap();
    let back = dir.path().join("back.png");
    process(
        &jxl,
        &back,
        &ProcessOptions {
            format: Some(Format::Png),
            preset: Preset::Lossless,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(image::open(back).unwrap().into_rgba8(), pixels);
}
#[test]
fn svg_import_rasterizes_and_gif_animation_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let svg = dir.path().join("input.svg");
    fs::write(&svg,br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><rect width="80" height="40" fill="red"/></svg>"#).unwrap();
    let png = dir.path().join("svg.png");
    process(
        &svg,
        &png,
        &ProcessOptions {
            format: Some(Format::Png),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        image::open(png).unwrap().into_rgba8().get_pixel(20, 20).0,
        [255, 0, 0, 255]
    );
    let external = dir.path().join("external.svg");
    fs::write(&external,br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><image href="file:///missing.png" width="20" height="20"/></svg>"#).unwrap();
    assert_eq!(inspect_file(external).unwrap_err().code, "svg_resources");
    let gif = dir.path().join("animation.gif");
    let mut file = fs::File::create(&gif).unwrap();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut file);
        encoder
            .encode_frame(image::Frame::new(sample(16, 16)))
            .unwrap();
        encoder
            .encode_frame(image::Frame::new(sample(16, 16)))
            .unwrap();
    }
    assert_eq!(inspect_file(gif).unwrap_err().code, "animation");
}
