# Dependency verification · 2026-11-09

## English

🟢 [Done] Official npm metadata reports no outdated packages. Numeric upstream tag comparison confirms Strata v0.2.12 (`146df3c468739147f520ac3816de806f71232571`) is still the latest stable tag; source checkout and distribution pin remain unchanged. Reviewed every direct workspace Rust dependency against crates.io: no direct upgrade is needed, including Tauri 2.12.5. The separately qualified MLX-LM 0.32.2 profile adopted in Soliton 0.1.5 and other model/runtime pins are retained.

🟢 [Done] Cargo.lock-only compatible maintenance: jiff 0.2.40 → 0.2.41, jiff-core 0.1.3 → 0.1.4, jiff-static 0.2.40 → 0.2.41, through serde_with's optional dependency graph. These packages are not selected in the active macOS workspace tree (`cargo tree --workspace -i jiff` prints nothing). No source/API adaptation is needed and no user-visible feature or performance change is claimed. Final Cargo dry-run proposes zero compatible changes; parent-constrained generations are not overridden.

🟢 [Done] Offline upstream contract, frontend production build, all-target workspace check and complete workspace tests passed, including all 162 core tests. Built the ad hoc signed Apple Silicon Soliton 0.1.5 app (97.18 MiB); distribution preflight passed 66 native files with no errors. [Preflight evidence](evaluation/dependency-distribution-20261109.json). Existing Lucide use-client bundler warnings remain. UI interactions, real GPU inference and full model qualification were not repeated for this inactive lockfile-only change; their 2026-11-08 validation remains historical evidence. No model weights were downloaded, inference engines started or ordinary conversation/settings files changed by this run. No worktree changes existed before this update.

⭕️ [Pending] Other enabled dependency configurations/platforms, another-Mac GUI, Developer ID/notarization and broader model qualification are not certified by this local run. Those pre-existing limits remain unchanged.

## 日本語

🟢 [Done] npm・Rust直接依存・Strata上流を公式情報と照合し、新たな直接更新はありません。Strata 0.2.12、Tauri 2.12.5、認定済みMLX-LM 0.32.2と既存モデル固定値を保持しました。

🟢 [Done] Cargo.lockのjiff系列3件のみ互換パッチへ更新しました。現在のmacOS有効依存ツリーには含まれず、機能変更や性能向上を主張しません。上流契約、フロントエンド、全ターゲット確認、Rust全workspaceテスト（コア162件）、アプリビルド、66ネイティブファイルの配布前検証が成功しました。更新残りはゼロです。モデル・会話・通常設定は変更せず、今回GPU推論は起動していません。

⭕️ [Pending] 他構成・別Mac・正式署名/公証などの既存未検証範囲は維持します。

## 简体中文

🟢 [Done] 已对照官方npm、crates.io及Strata标签；直接依赖没有新版本需要采用。保留Strata 0.2.12、Tauri 2.12.5、已认证MLX-LM 0.32.2与既有模型固定版本。

🟢 [Done] 仅更新Cargo.lock中的三项jiff兼容补丁。它们未进入当前macOS有效依赖树，不代表功能或性能变化。上游契约、前端、全目标检查、完整Rust workspace测试（162项核心测试）、应用构建及66个原生文件分发检查通过。无进一步兼容更新；未修改模型、普通会话或设置，未启动GPU推理。

⭕️ [Pending] 其他配置、其他Mac、正式签名/公证等原有限制保持不变。
