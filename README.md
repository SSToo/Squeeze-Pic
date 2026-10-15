# Squeeze Pic

**English** · [简体中文](README.zh-CN.md)

[![Desktop checks and packages](https://github.com/SSToo/Squeeze-Pic/actions/workflows/build.yml/badge.svg)](https://github.com/SSToo/Squeeze-Pic/actions/workflows/build.yml)

An offline desktop app for image compression, format conversion, and resizing, built with Tauri 2, React / TypeScript, and a Rust processing core. Currently a local preview release, version 0.2.2.

## Features

- Two separate tools: **Compress images** keeps the original format; **Convert images** lets you choose the output format.
- Eleven output formats: JPEG, PNG, WebP, AVIF, JPEG XL, HEIF, GIF, BMP, TIFF, ICO, and ICNS. SVG can be imported and rasterized. Availability varies by platform; see the [format support table (Chinese)](docs/FORMATS.md).
- JPEG: MozJPEG lossy encoding and lossless coefficient optimization for existing JPEG files.
- PNG: lossless encoding and oxipng optimization, preserving RGB values in transparent pixels.
- WebP: libwebp lossy and strict lossless encoding with transparency support.
- High quality, balanced, smaller file, custom quality, and strict lossless presets; smart, fast, and deep strategies.
- Original size, percentage, longest edge, width, height, or bounding dimensions. Resizing preserves aspect ratio and avoids upscaling by default.
- Batch queue, native drag and drop and file selection, output folders, stage progress, cancellation, and retries.
- A separate detail comparison dialog with split view, side-by-side view, and zoom. Advanced options start collapsed. English and Chinese UI, with neutral light / dark themes and a `#1E8FFD` blue accent.
- Rust optimizations: lossless PNG palette / grayscale / bit-depth reduction, sRGB lookup tables and transparency fast paths, and incremental selection of the smallest encoding candidate. See the [0.2.1 measurements (Chinese)](docs/PERFORMANCE.md).
- Version 0.2.2 adds content-aware PNG encoding paths, lossless TIFF Deflate and prediction, borrowed output buffers and reuse of decoded pixels, and streaming hashes for source checks. See the [second optimization report (Chinese)](docs/PERFORMANCE-0.2.2.md).
- EXIF orientation correction and RGB ICC conversion to sRGB. Metadata support varies by format.
- Originals are read-only, existing files are never overwritten, and temporary outputs are validated before commit. The original file hash is checked again before writing the result.

The frontend manages the interface and jobs; a separate Rust worker handles image processing. Running the app does not require Python, ImageMagick, or other image command-line tools.

## Development

Requires Node.js 22.13+, Rust 1.92.0, CMake, and a platform C/C++ toolchain. On macOS, install Xcode Command Line Tools. On Windows, install MSVC C++ Build Tools, LLVM / libclang, and WebView2.

```sh
npm ci
npm run tauri -- dev
```

The development port is 1421, separate from Squeeze PDF's port 1420. `npm run dev:web` previews the interface only; processing local images requires the desktop app.

## CLI

```sh
cargo run -p squeeze-pic-cli -- inspect "photo.png"
cargo run -p squeeze-pic-cli -- convert "photo.png" --output "photo.webp" --format webp
cargo run -p squeeze-pic-cli -- convert "photo.png" --output "small.webp" --options options.json
```

Include only the settings you want to override in `options.json`:

```json
{
  "format": "webp",
  "preset": "balanced",
  "strategy": "smart",
  "resize": { "mode": "longest", "value": 1920 },
  "metadata": "preserve",
  "background": [255, 255, 255]
}
```

`format` accepts `null` or `jpeg` / `png` / `webp` / `avif` / `jxl` / `heif` / `gif` / `bmp` / `tiff` / `ico` / `icns`. `preset` accepts `lossless` / `high` / `balanced` / `small` / `custom`; `strategy` accepts `fast` / `smart` / `deep`. Custom quality uses `quality` (1–100). Resize modes are `original` / `percent` / `longest` / `width` / `height` / `fit`; `fit` uses `width` and `height`. Quality and dimensions are independent settings; strict lossless disallows resizing.

Compression with the same format, original dimensions, and preserved metadata exports a file only if it is smaller. Format conversion, resizing, and metadata removal produce the requested output even when the file grows. The CLI writes a JSON report to stdout and progress events or errors to stderr.

## Checks and packaging

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
npm run check
npm run build:web

# Generate reproducible development fixtures
cargo run -p squeeze-pic-core --example make_samples

# macOS application
npm run tauri -- build --bundles app

# macOS application and DMG
npm run package:mac

# Windows installer; run on Windows
npm run tauri -- build --bundles nsis
```

The static JPEG XL build requires CMake and libclang. Use `CMAKE` to specify the CMake executable when necessary; on Windows, `LIBCLANG_PATH` can point to the directory containing `libclang.dll`.

Packaging builds a worker for the selected architecture, generates third-party notices, and bundles the required resources. Local packages are written to `target/release/bundle/`. Distribution signing and notarization require a separately configured product identity.

GitHub Actions checks and builds installers for Windows x64, Mac Apple silicon, and Mac Intel. Download build artifacts from Actions for ordinary commits. Pushing a `v*` tag matching the project version automatically publishes a GitHub Release with license archives and SHA-256 checksums. See the [build and release guide (Chinese)](docs/CI.md).

## Current limitations

- Each image is limited to 24 million pixels, with input and output files limited to 64 MiB each. There is also an estimated 1 GiB working-memory budget and a 120-second job timeout. The memory estimate is not a hard cap on every underlying allocation.
- Animation, CMYK, HDR, 16-bit images, and unsupported color configurations are not handled by the corresponding processing paths. Unsupported input fails explicitly instead of silently taking the first frame or reducing bit depth.
- Static GIF uses palette encoding; PNG does not offer lossy palette quantization. Lossless PNG encoding can store resized pixels; only **strict lossless** promises unchanged original pixels.
- Previews are limited to a 1600 px longest edge and do not provide full-resolution pixel inspection. Changing settings does not yet generate live encoding previews; comparisons show the original and the completed output.
- Lossless JPEG optimization currently requires metadata preservation. Cross-format processing retains supported ICC / EXIF / XMP data and reports metadata that cannot be mapped. XMP that may contain stale dimensions is removed after rotation or resizing, with a warning in the report.
- Local validation currently covers macOS Apple silicon. Installation regression checks on Windows and Intel Macs, and a larger corpus of real images, remain pending.
- The host cleans up temporary worker files after normal cancellation. Recovery of files left by a power loss or forced host termination is not implemented yet.

See the [format support table (Chinese)](docs/FORMATS.md) for per-format scope, the [development plan (Chinese)](PLAN.md) for remaining work, and the [validation record (Chinese)](docs/VALIDATION.md) for completed checks.

## Third-party materials

The JPEG coefficient module was adapted from the Apache-2.0-licensed Squeeze PDF for standalone image processing; its license and NOTICE are retained. Other dependencies retain their respective licenses. See [THIRD-PARTY.md (Chinese)](THIRD-PARTY.md) and the generated `third-party/` directory. A distribution license has not yet been selected for newly authored project code.
