# Third-party components

`crates/squeeze-pic-core/src/jpeg_lossless.rs` is adapted from the adjacent Squeeze PDF project (Apache-2.0). The file now adds standalone JPEG bitstream validation; the compression pipeline handles image metadata separately. See `licenses/Squeeze-PDF-LICENSE.txt` and `licenses/Squeeze-PDF-NOTICE.txt` for the original terms and attribution.

Primary processing components include:

| Component | Role | Declared upstream license |
| --- | --- | --- |
| image | Rust JPEG / PNG / WebP decoding and PNG encoding | MIT OR Apache-2.0 |
| MozJPEG Rust binding | JPEG encoding | IJG |
| mozjpeg-sys / MozJPEG | Native JPEG implementation | IJG AND Zlib AND BSD-3-Clause |
| oxipng | PNG optimization | MIT |
| webp | Rust libwebp binding | MIT OR Apache-2.0 |
| libwebp-sys | Native binding/build package | MIT; bundled libwebp has its own notices |
| moxcms | ICC color conversion | BSD-3-Clause OR Apache-2.0 |
| Tauri / React / Vite | Desktop shell and UI | See generated notices for exact resolved packages |

`node scripts/prepare-notices.mjs` collects the locked Cargo and npm package inventories, available license texts and native-code notices. It conservatively includes development and target-specific packages. Generated materials are bundled under `third-party/`; source archives are included for dependencies declaring MPL terms. This is a build record, not a claim that all combinations have been reviewed for public distribution.

No libimagequant, libheif, libjxl, PDF.js, lopdf, fonts, or remotely hosted preview assets are included in this initial version.

## 0.2.0 新增格式

新增 ravif / rav1e、GIF / TIFF、icns、resvg / usvg / tiny-skia、jxl-oxide 与静态 libjxl（通过 MIT / Apache-2.0 的 jxl-sys）等依赖。通知脚本同时收集 libjxl、Highway、Brotli 的上游许可文本。HEIF / AVIF 解码使用 macOS ImageIO 系统框架，不捆绑第三方 HEVC 库。
