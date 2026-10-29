# Dependency update · 2026-10-29

## English

🟢 [Done] Reviewed every direct Rust/npm dependency against official crates.io/npm metadata and updated all workspace manifests and lockfiles to the latest stable compatible graph. Already-current packages were retained. Direct major migrations: reqwest 0.12.28 → 0.13.5, tower-http 0.6.11 → 0.7.1, rusqlite 0.32.1 → 0.41.1, sha2 0.10.9 → 0.11.1. Tauri stays on stable 2.12.5; tauri-build 2.7.4; dialog 2.8.3 → 2.8.4. Lucide 1.61.0 → 1.63.0. React 19.3.1, Vite 8.3.5, TypeScript 7.0.2, Playwright 1.64.1 and the remaining npm packages were already current. Manifest minimums now reflect the verified versions. npm outdated returned `{}`. Transitive crates remain within their upstream parents' constraints; incompatible duplicate generations are not forcibly overridden.

🟢 [Done] Reqwest 0.13 uses the `rustls` feature in place of `rustls-tls`; its resolved graph includes the AWS-LC crypto provider and platform certificate verifier. SHA-256 output arrays no longer implement LowerHex: the existing Omni file/tree identity now formats each byte as two lowercase hex digits. A known SHA-256 vector and zero-padding check protect the existing identity representation. The existing Omni request, admission, history, exclusive lifecycle and pinned-runtime work was retained, including its previously uncommitted sha2 dependency. SQLite remains bundled; catalog/database regression tests pass.

🟢 [Done] Official Strata v0.2.6 → v0.2.7.1 (`46c152a1310f77cd2f0ce2c7cd213962df14fe83`), source version 0.1.53. Reviewed Mac setup, lifecycle and server changes. No new model family or vision capability is enabled by updating the source. Its dedicated Python environment now matches upstream requirements.txt and requirements-build.txt for Python 3.10: certifi 2026.7.22 → 2026.10.19, truststore 0.10.5 added; other specified packages already matched. pip check passed. Python-version-specific NumPy 2.2.6 remains the official Python 3.10 pin; model-qualified Python environments/interpreters are not mechanically upgraded. Metal rebuilt at `Strata/build-metal-v024-make/strata`; digest `7ee8edd54e7157e7d0d305ba57016201cbb3922ba4b6170ee6e5b2c10400226d`. No model download or ordinary model settings change occurred.

🟢 [Done] Verification: frontend build; offline upstream contract; workspace check; 86 Rust core tests including Omni and hash compatibility; 16 benchmark tests; context-matrix tests; 5 cache tests; token-counter test. Upstream server/Responses: 399 passed / 1 skipped of 400. Upstream Mac setup/speed/lifeline: 41 passed / 1 skipped of 42. Existing upstream ResourceWarnings were emitted, but these runs had no failing tests. The initial Python test command used the wrong working directory and was rerun successfully from Strata. Three-language engine switch, menus, model inventory, supervised local reuse without weight downloads, split docking and floating fixtures passed. Menus/models/reuse cover four widths; split docking covers 1440/390/320 px and light/dark. Native IPC is emulated in floating fixtures; native OS pointer drag is not covered. Browser skill unavailable, so repository Playwright with installed Chrome was used. Screenshots live outside the repository at /private/tmp/soliton-update-desktop-20261029.png and /private/tmp/soliton-update-narrow-20261029.png. Existing Lucide use-client bundler and upstream C++ deprecation/override/duplicate-library warnings remain.

⭕️ [Pending] Strata real GPU inference after this update: an existing 30-minute Omni qualification (PID 39165 with owned llama PID 39258 at the check) is still active. Its processes, runtime binaries, model/projector hashes and evidence gates were left untouched. Do not treat the previous v0.2.6 real response as certification of v0.2.7.1. vLLM-Apple and Omni pinned model runtimes are independent qualification contracts, not ordinary workspace libraries to advance during a running campaign. Native interactive validation, other target operating systems, and broader model qualification remain unverified.

🟠 [Next] Once the other qualification releases the GPU, run authenticated Strata inference/local re-preparation using existing assets and migrate old saved StrataStudio paths separately. Requalify any future changes to the pinned model runtimes before adopting them. Shared Document/state and Rust-owned media/GPU boundaries remain unchanged.

## 日本語

🟢 [Done] SolitonのRust/npm直接依存をすべて公式の最新安定版と照合し、manifestとlockfileを更新しました。reqwest 0.13.5、tower-http 0.7.1、rusqlite 0.41.1、sha2 0.11.1へメジャー移行し、Lucide 1.63.0、Tauri dialog 2.8.4、Strata 0.2.7.1へ更新。Tauri本体は最新2系の2.12.5を維持します。npm outdatedは空です。上流が必要とする旧世代の間接依存は強制上書きしていません。

🟢 [Done] SHA-256の64桁小文字形式を維持する互換修正と既知ベクターテストを追加し、進行中のOmni実装を保持しました。Strata専用Python依存を上流指定に揃え、Metalを再ビルド。Rust 86テスト、上流API 399成功/1スキップ、Mac関連41成功/1スキップ、3言語のエンジン・メニュー・モデル一覧・再利用・分割/フローティング回帰が成功しました。

⭕️ [Pending] 別のOmni実モデル認定がGPUを使用中のため、新Strataの実推論は未認定です。認定対象のランタイム・モデル・Python環境を一括更新して根拠を無効化することは避け、それらの固定値は保持しました。通常の会話・設定とモデルファイルは変更していません。

🟠 [Next] GPU解放後にStrataの認証付き実推論とローカル再準備を確認し、保存済み旧パス移行を別途完了します。

## 简体中文

🟢 [Done] 已逐项对照官方最新稳定版，更新Soliton所有直接Rust/npm依赖的清单与锁文件。主要迁移为reqwest 0.13.5、tower-http 0.7.1、rusqlite 0.41.1、sha2 0.11.1，并更新Lucide 1.63.0、Tauri dialog 2.8.4、Strata 0.2.7.1。Tauri保留最新2系2.12.5。npm outdated为空；没有强制覆盖上游依赖的旧代传递版本。

🟢 [Done] 修复SHA-256格式兼容性并用已知向量验证，保留正在开发的Omni实现。Strata专用Python环境已对齐上游固定依赖，Metal重新构建成功。86项Rust测试、399项上游API测试（另1项跳过）、41项Mac相关测试（另1项跳过），以及三语言引擎、菜单、模型库、本地复用和停靠测试均通过。

⭕️ [Pending] 另一个Omni真实模型认证仍在使用GPU，新Strata真实推理尚未认证。保留其固定运行时、模型、Python环境及证据门禁；未更改普通会话、设置或模型文件。

🟠 [Next] GPU释放后验证Strata认证真实推理和本地重新准备，并另行迁移保存配置的旧路径。

[Machine-readable versions and checks](evaluation/dependency-update-20261029.json). Sources: [reqwest documentation](https://docs.rs/reqwest/0.13.5/reqwest/), [official Strata tag](https://github.com/Niko1221/Strata/releases/tag/v0.2.7.1).

🟢 [Done] Final unsigned Apple Silicon Soliton.app build passed (6m46s release compilation, 12.54 MiB bundle). `git diff --check` passed. 日本語：更新済み依存で署名なしアプリのビルド成功。简体中文：更新依赖的未签名应用构建成功。
