# Shuo

Shuo 是一个 macOS 菜单栏语音转写 app。

- UI: Swift / AppKit / SwiftUI
- Engine: Rust
- IPC: stdio JSONL
- 平台: macOS 13+

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

### 安装到本机

- `bash Packaging/install_shuo_app.sh`
- 默认安装到：`~/Applications/Shuo.app`

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
