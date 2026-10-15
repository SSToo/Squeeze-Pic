# GitHub 自动构建与发布

仓库：[SSToo/Squeeze-Pic](https://github.com/SSToo/Squeeze-Pic)。工作流：[Desktop checks and packages](https://github.com/SSToo/Squeeze-Pic/actions/workflows/build.yml)，配置位于 `.github/workflows/build.yml`。

每次 push、Pull Request 和手动运行均执行版本检查、发布脚本测试、TypeScript、Rust 格式检查、Clippy 和工作区测试，再构建三个平台：

| 平台 | Runner | 下载文件 |
| --- | --- | --- |
| Windows x64 | windows-latest | NSIS `.exe`，含离线 WebView2 安装组件 |
| Mac Apple silicon | macos-15 | arm64 `.dmg` |
| Mac Intel | macos-15-intel | x64 `.dmg` |

打开 Actions 中一次成功的运行，在 **Artifacts** 下载对应 `squeeze-pic-目标架构`。每份包含安装包、第三方许可归档、构建信息和 `SHA256SUMS.txt`，保留 14 天。Mac DMG 打包时验证完整性并检查应用签名；所有平台检查实际 worker 版本。

JPEG XL 所需的 CMake、Windows LLVM/libclang 和平台 C/C++ 工具链在构建步骤检查或定位；worker 与桌面应用使用相同目标架构。Cargo 与 npm 使用锁文件，Rust 固定为 1.92.0，Node 为 22，启用依赖缓存。首次原生编译最多使用 90 分钟构建预算，失败时也保存可复用的 Rust 缓存。

同一分支的普通 push 会等待正在执行的安装包构建完成，再运行最近一份待构建提交，保留完整构建及其缓存。Pull Request 的过时构建会自动取消。

## 版本发布

当 push `v` 开头的版本标签时，标签必须与 `package.json`、`package-lock.json`、Cargo workspace / lock 和 Tauri 配置全部一致。三个平台都成功后，工作流核对下载产物的 SHA-256 与构建来源，创建 GitHub Release 草稿、上传全部安装包与许可归档，验证上传大小与摘要后自动发布。预发布版本标签（例如 `v0.3.0-beta.1`）标为 prerelease。

例如在所有版本文件已经设为 `0.2.2` 时：

```sh
git tag v0.2.2
git push origin v0.2.2
```

普通提交和手动运行只生成 Actions 产物。发布使用 job 范围的 `GITHUB_TOKEN`，不需要配置额外 PAT。相同内容可以重跑；已经存在但字节不同的同名资产会失败，不覆盖旧安装包，应使用新版本标签。

本地检查发布逻辑：

```sh
node scripts/release.mjs check-version v0.2.2
node --test scripts/test-release.mjs
```

CI 构建成功不等同于安装和界面回归。当前仍采用 Mac ad-hoc 签名、Windows 未签名；Apple Developer ID / 公证、Windows 代码签名及应用内自动更新尚未配置。

参考：[GitHub 托管 runner](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)、[Tauri GitHub 构建指南](https://v2.tauri.app/distribute/pipelines/github/)。
