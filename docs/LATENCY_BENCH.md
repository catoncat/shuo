# Latency Bench

- 日期：2026-04-07
- 命令：`swift run shuo latency-bench --runs 3 --chunk-ms 10,20,40`
- 语音：`Tingting`
- 文本：`现在我们直接测首字延迟，看看哪个参数最快。`
- 原始结果：`docs/latency-bench/latest.json`

## 真实 App 基线

- 日志：`~/Library/Application Support/shuo/diagnostics/timeline/runtime-1775534502976-pid11948.jsonl`
- 最近 20 个会话：
  - `first_result_frame_ms`：`p50=1540.5` `p95=1889.6` `min=1040` `max=2053`
  - `infer_ms`：`p50=214.0` `p95=258.5` `min=147` `max=420`

## Benchmark 排名

| rank | profile | chunk_ms | first_result_after_audio p50 | p95 | first_result_frame p50 | infer p50 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | `current-opus` | 20 | 781 | 788.2 | 1091 | 220 |
| 2 | `current-opus` | 10 | 784 | 792.1 | 1082 | 202 |
| 3 | `current-pcm` | 20 | 786 | 789.6 | 1082 | 198 |
| 4 | `current-pcm` | 40 | 790 | 806.2 | 1090 | 245 |
| 5 | `android-opus` | 20 | 792 | 797.4 | 1166 | 202 |
| 6 | `current-opus` | 40 | 798 | 801.6 | 1194 | 244 |
| 7 | `android-opus` | 40 | 799 | 806.2 | 1187 | 207 |
| 8 | `current-pcm` | 10 | 801 | 807.3 | 1098 | 233 |
| 9 | `android-opus` | 10 | 802 | 802.0 | 1177 | 217 |

## 结论

- 最优参数：`current-opus + chunk_ms=20`
- 如果只看当前 live runtime 兼容路径：`current-pcm + chunk_ms=20`
- `android-pcm`：3/3 全失败，服务端返回 `SessionFailed 50700000`
- 已应用：
  - app / `stdio-engine` 默认 `frontier_profile` -> `current-opus`
  - live transport 默认 20ms frame
  - release 产物：`dist/Shuo.app` / `dist/Shuo.app.zip`

## 当前限制

- 这次最优结果来自 benchmark replay harness，不是 live mic runtime。
- 现已把 app / stdio-engine 默认 profile 切到 `current-opus`，仍需继续观察真实 mic 会话的稳定性与首字收益。
