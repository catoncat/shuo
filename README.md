# Shuo

Shuo 是一个 macOS 菜单栏语音转写 app。

- UI: Swift / AppKit / SwiftUI
- Engine: Rust
- IPC: stdio JSONL
- 平台: macOS 13+

## 下载

- 直接下载：
  - GitHub Releases 里的 `Shuo-vX.Y.Z-macos.zip`
  - https://github.com/catoncat/shuo-app/releases
- 解压后把 `Shuo.app` 拖到 `Applications`
- 首次打开如被拦截：
  - 系统设置 → 隐私与安全性 → 仍要打开

## clone 后怎么用

### 依赖

- Xcode Command Line Tools
- Rust toolchain

### 本地运行

- 构建调试版：
  - `swift build`
  - `cargo build --manifest-path Engine/shuo-engine/Cargo.toml`
- 直接运行菜单栏 app：
  - `swift run shuo app`

### 打包成 `.app`

- `bash Packaging/build_shuo_app.sh`
- 产物：`dist/Shuo.app`
- 发布压缩包：`bash Packaging/package_release.sh v0.1.0`
- GitHub Releases 会附带：
  - `Shuo-vX.Y.Z-macos.zip`
  - `Shuo-vX.Y.Z-macos.zip.sha256`

### 安装到本机

- `bash Packaging/install_shuo_app.sh`
- 默认安装到：`~/Applications/Shuo.app`
- 也可以直接从 GitHub Releases 下载 zip，解压后拖入 `Applications`。

## 首次使用

- 首次打开需要授权：
  - 麦克风
  - Accessibility / Automation
- 配置文件主路径：
  - `configs/shuo.context.json`
- 运行时缓存目录：
  - `~/Library/Application Support/shuo/`
  - `~/Library/Application Support/shuo-engine/`

## 认证说明

Shuo 内置多种 frontier auth 获取路径，优先使用可直接 materialize 的本地缓存/会话材料。
默认构建目标是 standalone 使用，不要求依赖 `hj` research repo。

## 开发

- Swift host: `App/Sources`
- Rust helper: `Engine/shuo-engine`
- Shared contract: `Shared`
- Packaging: `Packaging`
- GitHub Release:
  - push tag：`git tag v0.1.0 && git push origin v0.1.0`
  - 或手动触发 `.github/workflows/release.yml`
