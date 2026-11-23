# Dependency update · 2026-11-23

- 🟢 [Done] Cargo updates: hashbrown 0.17.2, jiff/jiff-static 1.0.2; npm has no stable update. Tauri stays on version 2. The 183 Rust tests, flat/split contracts and three-language engine/preset fixtures passed.
- 🟢 [Done] Strata 0.2.20 candidate (`a34979fc94f3d126b9bb8210f4ad311345688b52`) rebuilt Metal source 0.1.66 and passed authenticated existing-Q2_0 inference, unauthorized 401 and clean shutdown.
- 🟢 [Done] Workspace/docking/floating fixtures and the final macOS build passed. Fixed composer clipping in short split slots while retaining the conversation sidebar. Distribution verification checked 66 native files and relocated Python imports without errors. [Report](evaluation/dependency-distribution-20261123.json).
- 🟢 [Done] The original tests pass 46/47 on pristine source and 45/47 with Soliton's prebuilt-engine overlay. The compatibility runner checks the actual dispatcher and each build's compiler requirement in place of the two stale assertions. Both variants pass 47/47. [Compatibility runner](../scripts/upstream-compatibility.py)
- 🟢 [Done] The standard Soliton 0.1.15 DMG with Strata 0.2.20 passed notarization, ticket validation, Gatekeeper and signed-runtime Q2_0 migration. [Final distribution verification](evaluation/strata-production-notarization-20261123.json)
- 日本語：未加工・同梱用の両ソースで修正後の47件が通過しました。0.2.20を同梱した通常版DMGも公証とGatekeeperに合格しました。
- 简体中文：修正后，原始源码和打包源码均通过47项测试。内置0.2.20的正式DMG也通过了Apple公证和Gatekeeper验证。

# Dependency update · 2026-11-21

- 🟢 [Done] Strata v0.2.15 (`c77572806996349b49995e4f98d41b5294d36a21`), rebuilt Metal source 0.1.61; Tauri Rust 2.12.7 / JS API 2.12.5 / CLI 2.12.6; Lucide 1.77.0; rusqlite 0.43.0; desktop base64 0.23.1 and compatible Cargo updates.
- 🟢 [Done] 178 Rust tests, upstream contract, 43 upstream Mac setup tests, seven distribution checker tests, three-language engine switching, workspace/docking fixtures and LaTeX checks passed. Existing Q2_0 returned an exact authenticated response and shut down cleanly.
- 🟢 [Done] Apple Silicon desktop build, signature integrity and relocated Python smoke passed. Distribution preflight checked 66 native files without errors. [Evidence](evaluation/dependency-distribution-20261121.json).
- 🟠 [Next] Strata v0.2.17 requires a coordinated `terminal/` + `engine/` layout migration. The current pin is the validated intermediate v0.2.15 release.
- ⭕️ [Pending] Full model re-preparation, another-Mac install and native pointer dragging remain unverified. [Versions, evidence and limits](DEPENDENCY_UPDATE_20261121.md).
- 日本語：Strataは0.2.15まで段階更新し、Metal再ビルドと既存Q2_0の実推論を確認しました。0.2.17の配置移行とモデル再準備全体は未完了です。
- 简体中文：已分阶段更新至Strata 0.2.15，Metal重新构建和已有Q2_0真实推理通过。0.2.17目录迁移及完整模型准备尚未完成。

# Dependency update · 2026-11-16

- 🟢 [Done] Strata v0.2.14.1 (`3af562d541ec76c8da63d568248a919c8a54906b`), Metal source 0.1.60.1. Source and distribution pins match. The bundle now uses upstream vendored ggml/GGUF; its real quant header is installed under the existing compatibility filename.
- 🟢 [Done] Six compatible Cargo updates (Brotli, Brotli decompressor, Jiff trio, rand). npm and Tauri 2 were unchanged. Frontend build, workspace check and 174 Rust tests passed (one ignored); the upstream contract, 54 focused upstream tests (one skipped), seven distribution checker tests and three-language engine-switch fixtures passed.
- 🟢 [Done] Existing Q2_0 passed authenticated exact-response inference, HTTP 401 without credentials and clean shutdown on the rebuilt Metal engine, with a temporary configuration. [Evidence](evaluation/strata-real-20261116.json).
- 🟢 [Done] Final Apple Silicon distribution preflight passed: 66 native files, no errors.
- ⭕️ [Pending] Full model re-preparation and installed-version migration were not repeated. No model download or ordinary settings changes.
- 日本語：Strata 0.2.14.1とRust間接依存6件を更新し、同梱ggmlへの移行に対応。既存Q2_0で実推論に成功。モデル再準備と旧版からの実機移行は未検証です。
- 简体中文：已更新Strata 0.2.14.1和六项Rust传递依赖，适配内置ggml。已有Q2_0真实推理通过；未重新准备模型或实测旧版安装迁移。

# Dependency update · 2026-11-15

- 🟢 [Done] Strata source and distribution pin: v0.2.13.1 (`628f74955a56fcba092495b800dd02af75373a0f`); local Metal build source version 0.1.59.1. This is a source build, not installation of the upstream Linux-only Control artifact for this tag.
- 🟢 [Done] Lucide React 1.73.0, Playwright 1.65.1, Prettier 3.10.1, Vite 8.3.8 and compatible Cargo lockfile updates (including UUID 1.29.2 and Wry 0.57.2). Tauri remains 2.12.5. The npm registry reported no remaining updates.
- 🟢 [Done] Frontend build, all-target Cargo check, 174 workspace tests (one ignored), offline upstream contract, 54 focused upstream tests (one skipped), and three-language Strata/vLLM-Apple switching fixtures passed. Existing diagnostic code, roadmap edits and evaluation data were preserved.
- 🟢 [Done] Existing Q2_0 passed authenticated exact-response inference, unauthenticated 401 and clean shutdown using the rebuilt Metal engine and temporary configuration. [Verification](evaluation/strata-real-20261115.json). Full model re-preparation was not repeated.
- 🟢 [Done] Apple Silicon app build and local distribution preflight passed: 66 native files, no errors, matching Strata pin. [Evidence](evaluation/dependency-distribution-20261115.json).
- 日本語：既存Q2_0の認証付き実推論と正常停止を確認しました。今回は再準備を再実行していません。Strataのソースと配布ピンを0.2.13.1へ同期し、npm/Rust依存を更新しました。既存の診断コード、ロードマップ差分、評価データは保持しています。
- 简体中文：已有Q2_0的认证真实推理及正常停止通过；本次未重新准备模型。已同步更新Strata源码和分发固定版本至0.2.13.1，并更新npm/Rust依赖；保留已有诊断代码、路线图修改和评估数据。

> 2026-11-09: Strata 0.2.12 and its pin are unchanged. [Weekly lockfile verification](DEPENDENCY_UPDATE_20261109.md). 日本語：上流固定値は変更なし。简体中文：上游固定值未变更。

> Follow-up · 2026-11-02: the disk-space Pending item below is resolved. [Cache cleanup, real Q2_0 re-preparation and data-preservation verification](CACHE_CLEANUP_REPREP_20261102.md). 日本語：容量不足による保留を解除しました。简体中文：容量不足导致的待验证项已完成。

# Current dependency update · 2026-11-02

Strata v0.2.12 (`146df3c4`), source engine 0.1.58. [Distribution pin, real inference and validation limits](DEPENDENCY_UPDATE_20261108.md). Change both the Strata gitlink and `src-tauri/strata-revision.txt` for future updates. Earlier sections are historical.

# Current dependency update · 2026-10-29

Strata v0.2.7.1 (`46c152a1`), source engine 0.1.53; [current checks and GPU qualification limits](DEPENDENCY_UPDATE_20261029.md). Earlier sections are historical snapshots.

# Current source update · 2026-10-26

Strata is now v0.2.6 (`4b9e05a2`), source engine 0.1.51. See [latest update, real-inference evidence and limits](DEPENDENCY_UPDATE_20261026.md). Earlier sections are historical validation snapshots.

# Current source update · 2026-10-24

Strata source is now v0.2.4 (`96ca4383`). See [current dependency update and validation limits](DEPENDENCY_UPDATE_20261024.md). Real inference results below apply only to the historical 0.2.1 installation.

# Upstream compatibility · 2026-10-21

Studio's `Strata` gitlink now pins `9de04cd44e927a9a3df6bbc0e94fc1be8b844ba4` from [Niko1221/Strata](https://github.com/Niko1221/Strata/commit/9de04cd44e927a9a3df6bbc0e94fc1be8b844ba4). This is the Strata 0.2.1 line, with engine 0.1.46 and the Control-only source-version patch 0.1.46.3. The local Metal build's `BUILD.json` reports 0.1.46.3. The previous gitlink was `f85fbb151bd11542d686ca1f35ee22b7bc5e3f05`; that local commit remains on its existing branch. Its tracked Metal headers were retained as a local build dependency. Upstream tracked sources were not patched.

`.gitmodules` records the upstream URL so a fresh clone can initialize the pinned checkout:

```sh
git submodule update --init Strata
npm run test:upstream
cargo test -p studio-core
```

## Integration changes

- Mac setup now imports planning from `strata_control.mac_fit`. Studio disables optional MTP fetching both in the setup module and in the shared planner. This includes resident/large-memory profiles; disabling only the old alias could leave `--mtp` in the generated arguments without preparing those weights.
- `data_folder_for` passes four migration/default-folder arguments. Local reuse accepts both old and new calls and returns only its owned workspace. It never migrates another app's files or saves a new upstream default folder.
- Runtime and build dependencies are now separate upstream. The official builder supplies build tools when necessary. Studio retains the dedicated `.venv` and invokes the source builder; existing model files are reused.
- New `serve.lifeline` starts the native engine in its own process group. Mac memory sampling now follows the owned server's child/descendant relationships, including the native engine and its lifecycle watcher, instead of assuming they share a group. Unrelated process trees are excluded. Existing metadata fields remain compatible. Shared allocations may still be charged more than once; this is a sum of physical footprints, not unique pages.
- Studio's authenticated loopback launch and existing chat API remain in use. The new upstream desktop/Terminal installer is not launched. Image/vision preparation remains explicitly off in Studio's current setup flow.

## Validation and limits

- The real new Mac setup module passes the offline local-reuse contract check for 32/128 GiB, with no MTP arguments, no model downloads and no data migration. The wrapper remains compatible with legacy setup calls.
- 56 Studio Rust tests pass, including the four-argument preparation contract and a native child started in a separate process group. Upstream Mac setup's 24 tests pass.
- Both the Metal engine and Studio's Apple Silicon app build successfully. Existing Q2_0 shards and their prepared pack are reused. No model download occurred.
- Upstream `serve.test_server` and `serve.test_responses`: 384 tests ran; 382 passed, one skipped, one VisionArgs fixture errored on macOS. Its fake `Proc` has no `pid`, but the new lifeline watcher expects one. This is an upstream test-fixture limitation, not a successful vision test; upstream sources were left unchanged. Studio keeps vision disabled. GPU backends other than Metal and additional model families were not validated here.

For future upstream updates, run the offline contract first, rebuild Metal from the new source rather than keeping an old executable, then exercise local-file preparation and authenticated real inference. Preserve the selected memory settings, conversations and stopped/running state. Do not infer compatibility with new model families solely from upstream support.

Final real inference on Apple M4 / 32 GiB: the existing Q2_0 returned the exact synthetic verification code, 35 input / 8 output tokens, in 10.864 seconds. Twelve memory samples each counted three processes (server, native engine and watcher); sampled footprint peaked at 7.63 GiB. These short-request samples are not a performance benchmark of the release. [Raw measurement](benchmarks/metal-upstream-20261021.json). The native Engine tab showed the summed memory, then “—” after stopping. The six-message document, conversation metadata, layout and Studio settings matched their pre-test snapshot. The 8K context, prompt chunk 64 and 7 GB reserve were restored after upstream re-preparation by Studio’s existing policy.
