> Follow-up · 2026-11-02: the disk-space Pending item below is resolved. [Cache cleanup, real Q2_0 re-preparation and data-preservation verification](CACHE_CLEANUP_REPREP_20261102.md). 日本語：容量不足による保留を解除しました。简体中文：容量不足导致的待验证项已完成。

# Dependency update · 2026-11-02

## English

🟢 [Done] Official Strata v0.2.7.1 → v0.2.9, commit `c62d5e3a15b40ea51c96aae9e8d36928965f53ab`, source engine 0.1.55. Lucide 1.63.0 → 1.66.0; tokio-tungstenite 0.28.0 → 0.30.0; direct base64 0.22.1 → 0.23.1. Cargo refreshed 11 compatible packages initially (including log, crc32fast, utf8_iter and wasm-bindgen), then resolved the WebSocket/SHA-1 graph for the major migration. Tauri remains on stable 2.12.5. npm outdated returned `{}`. Parent-constrained transitive generations and separately qualified model/runtime pins are retained.

🟢 [Done] Distribution correctness fix: the old source bundler and Metal builder used `HEAD:Strata`, so an upstream update in the current working tree could still ship the previous committed engine. `src-tauri/strata-revision.txt` now explicitly pins the immutable official commit for both source and Metal builds. Python packaging requires the checkout and its requirements.txt to match that pin. Distribution preflight compares the bundled Metal marker to the same pin and rejects stale/missing/invalid revisions. Future updates must change both the Strata gitlink and this pin. Upstream tracked sources were not patched. The user's pre-existing .gitignore changes were retained.

🟢 [Done] Rebuilt the actual distribution Metal engine for macOS 15.0; source digest `5b58a57c91f9e0978464a374998eeeb5141fa3208d12badfaeabb7becfebfce6`. Existing pinned Gemma backend also rebuilt successfully. Bundled Python remains 3.12.15 with official upstream runtime requirements (including NumPy 2.5.3) and sentencepiece 0.2.1. No model weights were downloaded. Tauri produced a 144.02 MiB Apple Silicon app with an ad hoc integrity signature. Local distribution preflight passed all 71 native files, minimum macOS/relocatable dependencies/checksums and bundled Strata commit. The relocated Python import smoke passed with a restricted system-only PATH. [Distribution evidence](evaluation/dependency-distribution-20261102.json). Developer ID/notarization and another-Mac GUI approval remain pending.

🟢 [Done] Build and regressions: npm build, offline upstream contract, cargo check --workspace --all-targets --locked (including Realtime probe), 112 Rust core tests, 7 distribution-check tests, 29 upstream Mac setup tests. Upstream server/Responses: 402 total, 401 passed / 1 skipped. Upstream test subprocess ResourceWarnings remain, without failing tests. Engine switching covers Strata/vLLM-Apple, private auth, shared history/metrics and cleanup; menus, split docking, model inventory and floating fixtures passed EN/JA/ZH and narrow layouts. Floating fixtures emulate native IPC and do not certify OS pointer drag.

🟢 [Done] Authenticated real Q2_0 inference used the newly built distribution Metal executable and bundled Python on isolated port 18956. Existing GGUF files, dense pack and tokenizer were reused; only a temporary config relocated former StrataStudio paths. Memory/context settings were retained. Unauthenticated access returned 401; the exact response SOLITON112 passed with reasoning_effort=none, 24 prompt / 8 output tokens; owned shutdown exit code 0. Engine SHA-256 `6a9fcb413b8ed06ce6733dfafd1c40890e2709fe3f3b9fb996c3093419921213`. Timings were 15009.6 ms prompt / 1649.4 ms decode; this short request alongside builds is not a performance benchmark. Ordinary conversation/settings and model files were not modified. [Inference evidence](evaluation/strata-real-20261102.json).

⭕️ [Pending] Successful local-file re-preparation cannot be exercised with current disk capacity: about 12–13 GiB free versus the Q2_0 reuse workspace budget (112.0 − 66.4 = 45.6 GB, including the mandatory SSD expert copy/headroom). Initial reuse smoke timed out clicking a correctly disabled button. It now reads the real API disk plan, verifies disabled UI/warning/unchanged configuration when insufficient, and explicitly reports the success path as Pending; it does not bypass the guard. Missing-shard refusal, existing-config activation, capacity refusal, three languages and four widths passed. Core fixture preparation tests passed; they are not full-size model re-preparation proof. No unrelated files were deleted to create room.

🟠 [Next] After sufficient space is available, rerun successful local re-preparation. Persistent old-path migration and native/another-Mac validation remain separate from the temporary inference and browser fixtures. New model families, broader GPU backends, long-running performance and pinned Omni/vLLM/Gemma runtime upgrades need their own qualification.

## 日本語

🟢 [Done] Strata 0.2.9（ソース版0.1.55）、Lucide 1.66.0、tokio-tungstenite 0.30.0、base64 0.23.1へ更新し、互換範囲内のRust依存も更新しました。Tauri 2.12.5と認定対象のモデル固定値を維持しています。

🟢 [Done] 配布処理が古いHEADのStrataを取り込む問題を修正しました。明示的なコミット固定ファイルをソース・Metal・Python依存・配布検証で共有し、旧版混入を拒否します。112 Rustテスト、配布7テスト、上流Mac29テスト、上流API401成功/1スキップ、3言語UI・エンジン・ドッキング回帰が成功しました。

🟢 [Done] 実際の配布用Metalと同梱Pythonで既存Q2_0の認証付き実推論が完全一致し、未認証401と正常停止も確認しました。アプリの再ビルド、ad hoc署名、71ネイティブファイルと移設Pythonの配布前検証も成功。通常設定とモデルファイルは保持しました。

⭕️ [Pending] 空き容量が約12–13 GiBのため、45.6 GBを要するQ2_0再準備の成功経路は保留です。容量ガードの拒否を確認し、テストでも成功扱いにしません。他Mac GUI、正式署名/公証、ネイティブOSドラッグも未認定です。

🟠 [Next] 十分な空き容量確保後に再準備を再検証します。モデルは自動ダウンロードせず、無関係なデータも削除していません。

## 简体中文

🟢 [Done] 更新至Strata 0.2.9（源码版本0.1.55）、Lucide 1.66.0、tokio-tungstenite 0.30.0、base64 0.23.1，并更新兼容的Rust依赖。保留Tauri 2.12.5及已认证模型固定版本。

🟢 [Done] 修复打包过程可能仍使用旧HEAD中Strata的问题。源码、Metal、Python依赖和分发检查共用明确的提交固定文件，拒绝混入旧版。112项Rust测试、7项分发测试、29项上游Mac测试、401项上游API测试（另1项跳过）及三语言UI/引擎/停靠回归通过。

🟢 [Done] 使用实际分发Metal和内置Python复用现有Q2_0进行认证真实推理，精确匹配答案，并验证未认证401与正常停止。应用重新构建、ad hoc签名、71个原生文件和迁移Python检查通过，普通配置和模型文件保持不变。

⭕️ [Pending] 当前仅剩约12–13 GiB空间，Q2_0重新准备需要45.6 GB，因此成功路径保留待验证。容量保护的拒绝已验证，测试不会将其误报为成功。其他Mac GUI、正式签名/公证和操作系统原生拖动尚未认证。

🟠 [Next] 空间充足后重新验证准备流程；未自动下载模型或删除无关数据。
