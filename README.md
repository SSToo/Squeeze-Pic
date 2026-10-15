# Squeeze Pic

[![Desktop checks and packages](https://github.com/SSToo/Squeeze-Pic/actions/workflows/build.yml/badge.svg)](https://github.com/SSToo/Squeeze-Pic/actions/workflows/build.yml)

基于 Tauri 2、React / TypeScript 和 Rust 的离线图片压缩、格式转换与尺寸调整工具。当前为本地试用版本 0.2.2。

## 当前功能

- 两个独立入口：**压缩图片**保留原格式；**格式转换**选择目标格式。
- JPEG、PNG、WebP、AVIF、JPEG XL、HEIF、GIF、BMP、TIFF、ICO、ICNS 共 11 种导出格式；SVG 可导入转换。详见 [格式支持表](docs/FORMATS.md)。
- JPEG：MozJPEG 有损编码，已有 JPEG 的系数域无损优化。
- PNG：无损编码与 oxipng 优化；保持透明像素中的 RGB。
- WebP：libwebp 有损和严格无损编码，保留透明度。
- 高画质、均衡、更小、自定义质量与严格无损；智能、快速、深度策略。
- 原尺寸、百分比、最长边、宽度、高度、宽高边界；默认等比、只缩小。
- 多图片队列、原生拖放与文件选择、输出目录、阶段进度、取消和失败重试。
- 点击打开独立的细节对比弹窗，支持分割、并排和缩放；更多选项默认收起，中英双语与中性灰明暗主题。
- Rust 压缩优化：PNG 无损调色板/灰度/位深简化，sRGB 查表与透明混合快速路径，逐个筛选最小编码候选。见[优化对照记录](docs/PERFORMANCE.md)。
- 0.2.2：PNG 按内容与策略选择直接编码路径，TIFF 使用无损 Deflate 和预测，输出校验借用文件缓冲并复用特殊格式已解码像素，原件复核使用分块哈希。见[第二轮深度优化记录](docs/PERFORMANCE-0.2.2.md)。
- EXIF 方向校正、RGB ICC 转为 sRGB；元数据能力按格式支持表区分。
- 原件只读、不覆盖同名文件；临时输出校验后提交，提交前复核原件哈希。

前端只管理任务与界面，图片处理在独立 Rust worker 中完成。无需安装 Python、ImageMagick 或其他图片命令行工具。

## 开发运行

需要 Node.js 22.13+、Rust 1.92.0 、CMake 和平台 C/C++ 编译环境。macOS 使用 Xcode Command Line Tools；Windows 使用 MSVC C++ Build Tools 和 WebView2。

```sh
npm ci
npm run tauri -- dev
```

开发端口为 1421，避免与 Squeeze PDF 的 1420 冲突。`npm run dev:web` 仅用于界面预览，真实图片处理需要桌面应用。

## CLI

```sh
cargo run -p squeeze-pic-cli -- inspect "photo.png"
cargo run -p squeeze-pic-cli -- convert "photo.png" --output "photo.webp" --format webp
cargo run -p squeeze-pic-cli -- convert "photo.png" --output "small.webp" --options options.json
```

`options.json` 可只包含需要覆盖的设置：

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

`format` 为 `null` 或 `jpeg` / `png` / `webp` / `avif` / `jxl` / `heif` / `gif` / `bmp` / `tiff` / `ico` / `icns`，`preset` 为 `lossless` / `high` / `balanced` / `small` / `custom`，`strategy` 为 `fast` / `smart` / `deep`。自定义画质使用 `quality`（1–100）。尺寸模式为 `original` / `percent` / `longest` / `width` / `height` / `fit`；`fit` 使用 `width` / `height`。质量参数与尺寸独立，严格无损禁止缩放。

同格式、原尺寸、保留元数据的压缩只有更小才导出；格式转换、尺寸设置或元数据移除按请求导出，即使体积增加。CLI 标准输出为 JSON 报告，标准错误输出阶段事件或错误。

## 验证与打包

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
npm run check
npm run build:web

# 创建可重复的开发测试样本
cargo run -p squeeze-pic-core --example make_samples

# macOS 应用
npm run tauri -- build --bundles app

# macOS 应用和 DMG
npm run package:mac

# Windows 安装包，在 Windows 上运行
npm run tauri -- build --bundles nsis
```

JPEG XL 的静态构建需要可用的 `cmake` 命令；也可通过 `CMAKE` 环境变量指定其路径。

打包脚本编译匹配当前架构的 worker，生成第三方通知，并将所需组件放入应用。包位于 `target/release/bundle/`。发布签名与公证需另行配置产品身份。

GitHub Actions 已配置 Windows x64、Mac Apple silicon 与 Mac Intel 自动检查和安装包构建。普通提交可在 Actions 下载产物；推送匹配项目版本的 `v*` 标签后自动发布 GitHub Release，附带许可归档和 SHA-256 校验文件。详见 [自动构建说明](docs/CI.md)。

## 当前边界

- 单图最多 2400 万像素、输入/输出各 64 MiB；另有估算 1 GiB 工作预算和 120 秒任务超时。这不是对全部底层分配的硬内存限制。
- 动画、CMYK、HDR、16 位、特殊颜色配置暂不支持相应处理。未支持的输入明确失败，不默认抽首帧或降位深。
- 静态 GIF 提供调色板编码；PNG 不提供有损调色板量化。无损 PNG 编码也可以承载缩放后的像素；只有“严格无损”承诺原像素不变。
- 预览最长边 1600 px，不是全分辨率 100% 像素检查；参数修改后暂不实时编码候选，前后对比展示原图与已导出的成品。
- JPEG 无损优化暂时要求保留元数据。跨格式处理保留 ICC / EXIF / XMP 中已支持的数据；无法映射的元数据在报告中说明。旋转/缩放后可能包含旧尺寸的 XMP 会移除并报告。
- 当前完成 macOS Apple silicon 本机验证；Windows、Intel macOS 的安装回归和真实图片大样本回归仍待完成。
- 正常取消的 worker 文件由宿主临时目录清理；断电或强制结束宿主后的遗留目录恢复尚未实现。

当前新增格式的范围见 [格式支持表](docs/FORMATS.md)，动画与跨平台补验继续按 [开发计划](PLAN.md) 进行。实际验证记录见 [VALIDATION.md](docs/VALIDATION.md)。

## 第三方材料

JPEG 系数域模块移植自 Apache-2.0 的 Squeeze PDF，修改后用于独立图片处理；保留原许可与 NOTICE。其余依赖保留各自许可，见 [THIRD-PARTY.md](THIRD-PARTY.md) 与构建生成的 `third-party/`。新增自有代码尚未选择对外发布许可证。
