# Shared

- 放冻结契约与 fixture：
  - `Shared/Fixtures/ipc.v1.jsonl`
  - `Shared/Fixtures/context-config.v1.json`
- 双端 golden tests 已消费这些 fixture：
  - Swift:
    - `hj-voice shared-contract-check`
  - Rust:
    - `Engine/hj-dictation/src/config.rs`
    - `Engine/hj-dictation/src/engine_ipc.rs`
