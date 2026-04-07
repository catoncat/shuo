# Changelog

## v0.1.2 - 2026-04-07

- 把 latency bench 拆成独立 `shuo-bench` 可执行工具
- 正式 `shuo` app / release bundle 不再包含 latency bench 入口
- release 构建只编译 `shuo`，避免把 bench 一起打进正式产物
- release 版本号更新为 `0.1.2`

## v0.1.1 - 2026-04-07

- 增加开始/结束提示音选择
- 默认提示音改为 `Siri 开始（短） + Pop`
- 可选系统 Siri 音效与 `/System/Library/Sounds` 系统音效
- 增加 latency bench / current-opus 默认链路
- release 版本号更新为 `0.1.1`

## v0.1.0 - 2026-04-07

- 独立发布 `shuo`
- Swift host + Rust engine
- Finder `.app` 打包脚本
- GitHub Release 自动产出 `Shuo-vX.Y.Z-macos.zip`
