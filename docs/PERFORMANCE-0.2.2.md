# Rust 深度压缩优化验证（0.2.2）

日期：2026-10-14。环境：macOS Apple silicon、Rust 1.92.0、release 构建。对照版本为 0.2.1，优化版本为最终 0.2.2 安装包随带的 Rust worker。

本轮没有降低默认画质。主要收益来自消除重复编码和缓冲复制，以及补上 TIFF 的无损压缩。

## 实现

- **PNG 直接处理像素**：智能、深度策略将已有 RGBA 缓冲直接交给 `oxipng::RawImage`，省去先编码 PNG、再解码并重压缩的中间步骤。ICC、EXIF、XMP、文本和 pHYs 随新容器写入；透明像素中的隐藏 RGB 保持不变。
- **快速 PNG 按内容分流**：采样判断少色图，继续使用单次编码；全彩图使用原始像素路径，完全不透明时原地移除未使用的 alpha 通道，跳过调色板搜索。直接让所有图片走优化器曾使小图变慢，因此保留分流。采样只影响编码路线，输出仍完整解码校验。
- **TIFF 无损压缩**：旧版使用未压缩 TIFF。新版按快速 / 智能 / 深度选择 Deflate 级别 1 / 6 / 9，采用水平预测；深度额外比较无预测结果，保留更小候选。明确标记 straight alpha，保持透明像素中的 RGB。
- **减少大图内存复制**：候选结果检查借用已有编码缓冲；特殊格式已解码的像素按需转移所有权，严格无损仍保留原始像素供比较；BMP 编码直接借用 RGBA。原件提交前复核改用 64 KiB 分块计算 SHA-256，避免再次分配整个输入文件。
- **保留完整校验**：生成 PNG 在编码前记录处理后像素与 ICC，输出完整解码后核对。没有跳过解码、原件变更检查、大小限制或同名输出保护。

## 最终包实测

Rust 示例生成 768 × 512 的少色、全彩噪声及透明样本；内存项使用 2048 × 2048 图片。每项依次运行旧、新、新、旧、旧、新，各版本 3 次，以下为中位数。耗时由外部计时，包含 worker 启动、读取、处理、完整解码校验及提交；RSS 取 macOS `/usr/bin/time -l` 的进程峰值。

| 样本 / 操作 | 0.2.1 输出 B | 0.2.2 输出 B | 旧 / 新耗时 ms | 旧 / 新 RSS MiB |
| --- | ---: | ---: | ---: | ---: |
| 少色 BMP → PNG / 快速 | 3,726 | 3,726 | 37.4 / 44.1 | 12.9 / 13.0 |
| 少色 BMP → PNG / 智能 | 1,354 | 1,354 | 196.4 / 201.3 | 20.3 / 19.3 |
| 少色 BMP → PNG / 深度 | 930 | 930 | 616.0 / 623.8 | 20.2 / 18.3 |
| 全彩 BMP → PNG / 快速 | 405,391 | 318,096 | 231.8 / 60.0 | 13.4 / 14.9 |
| 全彩 BMP → PNG / 智能 | 260,533 | 260,533 | 1,012.0 / 850.2 | 23.3 / 20.3 |
| 透明 BMP → PNG / 智能 | 2,163 | 2,163 | 226.8 / 235.6 | 20.5 / 18.5 |
| 附 RGB ICC 的 WebP → PNG / 智能 | 1,846 | 1,846 | 170.9 / 174.9 | 19.0 / 19.6 |
| 少色 PNG → TIFF / 智能 | 1,573,094 | 11,635 | 15.3 / 16.4 | 14.4 / 11.6 |
| 全彩 PNG → TIFF / 智能 | 1,573,094 | 388,873 | 25.4 / 118.0 | 16.5 / 14.0 |
| 透明 PNG → TIFF / 智能 | 1,573,094 | 16,631 | 15.5 / 19.4 | 14.4 / 12.5 |
| 未压缩 TIFF → TIFF / 智能 | 1,573,094 | 388,873 | 19.9 / 117.4 | 17.6 / 16.0 |
| 4MP BMP → BMP / 移除元数据 | 16,777,338 | 16,777,338 | 206.6 / 198.6 | 121.8 / 73.8 |
| 4MP JPEG XL → BMP / 移除元数据 | 16,777,338 | 16,777,338 | 662.9 / 664.5 | 156.5 / 108.4 |

全彩 PNG 快速项比旧版输出再小 **21.5%**，耗时下降 **74.1%**；智能项输出大小相同，耗时下降 **16.0%**。两项 4MP 处理的峰值 RSS 分别下降 **39.4%** 和 **30.7%**，输出文件 SHA-256 与旧版完全一致。其余 PNG 项并非全部加速，少色与透明小图有数毫秒额外耗时，快速全彩图的 RSS 也略有增加。

TIFF 样本体积减少 **75.3%–99.3%**，但旧版没有压缩，不能将此解释为对已有压缩 TIFF 的普遍收益。全彩图压缩由约 25 ms 增至 118 ms，属于用更多计算换取无损体积缩减；智能策略的速度与体积取舍如表所示。

这些都是小规模合成样本，未覆盖真实照片大样本、所有格式组合或其他机器。3 次测量无法排除系统负载、缓存及进程启动波动。原始每次计时、RSS、编码路径、输入与输出 SHA-256 见 [JSON 记录](benchmarks/compression-0.2.2.json)。上一轮 PNG 存储简化及 JPEG 优化的对照保留在 [0.2.1 记录](PERFORMANCE.md)，两轮耗时的统计口径不同，不能直接比较。

## 验证与复测

- 最终源代码通过工作区 **28 项测试**、格式检查、全部 targets 的 Clippy（拒绝 warning）、TypeScript 检查和前端构建。
- 新增测试覆盖三种 PNG 策略的像素、隐藏 RGB、ICC / EXIF / XMP / 文本 / pHYs；快速 PNG 的内容分流；TIFF 三种策略的逐像素、Deflate 与 alpha 标记；BMP 的透明与隐藏 RGB。
- 上表实际调用最终 `.app` 中的 worker，确认 CLI 与应用均为 0.2.2。应用签名检查及 DMG 完整性检查通过。该包使用本地 ad-hoc 签名，未配置公开发行签名与公证。
- 每次 benchmark 都核对报告中的输入哈希，检查各版本重复输出一致，并确认原始样本未被修改；两项内存对照还要求新旧输出文件逐字节一致。

生成可复用样本：

```sh
cargo run -p squeeze-pic-core --release --example compression_bench -- current
cargo run -p squeeze-pic-core --release --example deep_samples
```

本机保留 `tmp/deep-benchmark/baseline-worker`（0.2.1）、`optimized-worker`（最终 0.2.2）及 `run.py`。使用 `python3 tmp/deep-benchmark/run.py` 可重跑同一对照并更新 JSON；Python 仅用于开发计时，应用图片算法及运行时均不依赖 Python。复测时不要同时编译。

实现位置：`crates/squeeze-pic-core/src/pipeline.rs`、`metadata.rs`、`codecs.rs`。配置依据：[oxipng RawImage 文档](https://docs.rs/oxipng/latest/oxipng/struct.RawImage.html)、[TIFF 压缩默认值](https://docs.rs/tiff/latest/tiff/encoder/compression/index.html)、[DeflateLevel](https://docs.rs/tiff/latest/tiff/encoder/compression/enum.DeflateLevel.html)；ICC 兼容约束见 [PNG iCCP 规范](https://www.w3.org/TR/png-3/#11iCCP)。
