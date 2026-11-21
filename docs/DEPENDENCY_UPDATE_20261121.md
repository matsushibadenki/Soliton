# Dependency update · 2026-11-21

## Validated update unit

- 🟢 [Done] Strata source and distribution pin move from v0.2.14.1 to v0.2.15, commit `c77572806996349b49995e4f98d41b5294d36a21`. Metal was rebuilt from that source; `BUILD.json` reports engine source 0.1.61. This is an intermediate stable release, not the latest Strata release.
- 🟢 [Done] Tauri Rust 2.12.7, tauri-build 2.7.6, JavaScript API 2.12.5 and CLI 2.12.6. Tauri remains on major version 2.
- 🟢 [Done] Lucide React 1.77.0, rusqlite 0.43.0 / libsqlite3-sys 0.40.0, and the desktop's base64 0.23.1. Cargo also refreshed compatible transitive dependencies. npm reported no remaining updates after installation.
- 🟢 [Done] The vLLM-Apple remote remains at `f4b4b0b1976ea98168334a4bf68bab220d8bf3eb`; no newer source commit was available. Its existing qualified runtime and Python environment were retained.

## Verification

- 🟢 [Done] `npm run test:upstream`, `npm run build`, `cargo check --workspace --locked`, and `cargo test -p studio-core --locked` passed. The Rust suite ran 178 tests with no failures or ignored tests.
- 🟢 [Done] Upstream Mac setup ran 43 tests; distribution-checker unit tests ran seven. Both suites passed.
- 🟢 [Done] `npm run desktop:build -- --bundles app` passed, including the post-build distribution check. Ad hoc signature verification and relocated Python imports with a restricted PATH passed; 66 native files were checked. [Distribution report](evaluation/dependency-distribution-20261121.json).
- 🟢 [Done] Rebuilt `studio-server` and reran `test:engines`, `test:workspace`, `test:docking`, and `test:floating`. These cover three-language engine switching, saved presets, shared conversation state, window-host routing, layout persistence and drag transport. Native OS pointer dragging remains outside the fixture's scope.
- 🟢 [Done] Existing LaTeX rendering passed checks for inline/display delimiters, literal code spans and incomplete/unsupported formula fallback.
- 🟢 [Done] Rebuilt Strata reused the existing Q2_0 weights and prepared pack. An isolated, authenticated request returned exactly `SOLITON121`; missing credentials returned HTTP 401; shutdown exited 0. The test used a temporary 512-token context with a 7 GB reserve. [Raw result](evaluation/strata-real-20261121.json).
- ⭕️ [Pending] Full local model re-preparation was not rerun. The local-only setup adapter contract passed, including disabled MTP fetching and blocked weight downloads. This does not certify a fresh preparation job.
- ⭕️ [Pending] Another-Mac installation, native pointer dragging, Developer ID signing and notarization require separate verification. Test-model inference did not alter ordinary conversations or settings.

## Next Strata migration

- 🟠 [Next] Migrate to upstream v0.2.17 (`12d491da4a6904eb49aaa6f2f63ec4b1a17582e3`). The v0.2.16+ tree moves `serve`, `setup.py`, `requirements-build.txt`, `strata_control` and preparation tools into `terminal/`, and the native/Metal sources into `engine/`. Soliton currently uses the older paths in its launcher, preparation adapter, model inspection, immutable archive builder, provisioning checks and distribution preflight. These entry points must migrate together and pass the same Metal rebuild, local-reuse, authenticated-inference and packaged-install checks before changing the source/distribution pin.
- 🔴 [Later] Requalify installed-version migration and the complete model re-preparation workflow against that split layout.

## 日本語

🟢 [Done] 既存レイアウトと互換性のあるStrata 0.2.15へ段階更新し、新ソースでMetalを再ビルドしました。Tauri 2、Lucide、SQLiteなどの依存を更新し、Rust 178件、上流Mac準備43件、配布チェッカー7件、エンジン切替・共有状態・ドッキング・LaTeXの検証を通過しました。既存Q2_0の認証付き実推論と正常停止も確認しました。

🟠 [Next] 最新Strata 0.2.17は `terminal/` と `engine/` への配置移行が必要です。今回は0.2.15までを検証済みの更新単位としています。⭕️ [Pending] モデルの再準備全体、別Macへのインストール、Developer ID署名・公証は未検証です。

## 简体中文

🟢 [Done] 分阶段更新到与现有目录布局兼容的Strata 0.2.15，并从新源码重新构建Metal。已更新Tauri 2、Lucide、SQLite等依赖，通过178项Rust测试、43项上游Mac准备测试、7项分发检查器测试，以及引擎切换、共享状态、停靠和LaTeX验证。已有Q2_0模型的认证真实推理和正常关闭均通过。

🟠 [Next] 最新Strata 0.2.17需要适配 `terminal/` 与 `engine/` 的新目录布局。本次已验证的更新范围止于0.2.15。⭕️ [Pending] 尚未重新执行完整模型准备、其他Mac安装、Developer ID签名及公证验证。

## Official sources reviewed

- [Strata 0.2.15](https://github.com/Niko1221/Strata/releases/tag/v0.2.15), [Strata 0.2.17](https://github.com/Niko1221/Strata/releases/tag/v0.2.17) and tagged source trees.
- [Tauri releases](https://github.com/tauri-apps/tauri/releases), [Lucide releases](https://github.com/lucide-icons/lucide/releases), npm's package registry and the crates.io index.
- [rusqlite 0.43.0](https://crates.io/crates/rusqlite/0.43.0) and [base64 API](https://docs.rs/base64/0.23.1/base64/).
