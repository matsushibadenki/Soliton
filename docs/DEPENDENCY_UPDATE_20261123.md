# Dependency update · 2026-11-23

- 🟢 [Done] Updated compatible Cargo dependencies: hashbrown 0.17.1 → 0.17.2, jiff 1.0.1 → 1.0.2, and jiff-static 1.0.1 → 1.0.2. Tauri remains on major version 2. npm reported no stable updates.
- 🟢 [Done] The vLLM-Apple checkout and remote HEAD both report `1a402ebd29164b7db587eea1e1c5e3436248a8c8`. Its untracked evaluation work was preserved.
- 🟢 [Done] Advanced the opt-in split Strata candidate from 0.2.17 to [0.2.20](https://github.com/Niko1221/Strata/releases/tag/v0.2.20), commit `a34979fc94f3d126b9bb8210f4ad311345688b52`. Candidate builder, source archive and distribution checker pins agree. Normal source/distribution pins remain at the validated Strata 0.2.15 commit `c77572806996349b49995e4f98d41b5294d36a21`.
- 🟢 [Done] Rebuilt candidate Metal source 0.1.66 with macOS 15.0 deployment target, leaving the normal bundled archive intact. Existing Q2_0 shards, expert sidecar and prepared pack returned exactly `SOLITON123` through authenticated HTTP; anonymous access returned 401 and shutdown exited 0. Original weight/sidecar sizes and modification timestamps matched their pre-test values. The isolated test used context 512 and the existing 7 GB reserve. [Build](evaluation/strata-candidate-build-20261123.json) · [Inference](evaluation/strata-candidate-real-20261123.json).
- 🟢 [Done] Contract checks now accept both flat and split source trees. Both source contracts passed; the pinned production upstream Mac suite passed 43 tests and the distribution-checker unit suite passed seven.
- 🟢 [Done] `npm run build`, `cargo check --workspace --locked` and `cargo test -p studio-core --locked` passed. Rust ran 183 tests with no failures or ignored tests. Engine switching, shared chats and saved presets passed the existing three-language browser fixture. Updated its memory-panel assertion to follow the existing Status tab, preserving the implemented UI placement.
- 🟢 [Done] Workspace, split docking and floating-host browser fixtures passed. The docking test exposed a short, narrow split where the conversation sidebar left insufficient height for the composer. Limited the sidebar height while retaining its scrollable contents; the composer and tab controls now fit at 1440/390/320 widths in EN/JA/ZH and light/dark modes. No conversation or file-attachment controls were removed.
- 🟢 [Done] Built the final normal Apple Silicon app after the layout fix. Signature integrity, 66 native-file audits and relocated Python imports with a restricted PATH passed. The packaged archive still pins Strata 0.2.15. [Distribution evidence](evaluation/dependency-distribution-20261123.json).
- 🟠 [Next] The unmodified 0.2.20 upstream Mac suite has one failing assertion among 47 tests. It expects an exact older source string for Darwin dispatch, while the new source also excludes Inkling. This is recorded as a failure, not a passed suite. Review the updated dispatcher coverage before production promotion.
- 🟢 [Done] Qualified automatic migration of the reviewed managed Q2_0 config with an existing pack in the candidate DMG. Kept model arguments, expert-profile resolution, legacy config and history. The new config was active before authenticated inference. [Automatic migration](evaluation/strata-split-migration-020-20261123.json).
- 🟠 [Next] Verify complete preparation without an existing pack before promoting the split candidate.
- 🟢 [Done] Installed the 0.2.20 candidate DMG into isolated app data with copied legacy settings. It provisioned the split runtime, preserved the selected legacy config and conversation, and answered through authenticated Q2_0 chat after an explicit switch that retained the expert-profile file. [Packaged test](evaluation/strata-split-dmg-020-20261123.json).
- ⭕️ [Pending] Full model re-preparation, another-Mac installation and native pointer dragging were not run. Public Developer ID signing/notarization is a separate distribution step; it is not needed for local candidate validation.
- 🔴 [Later] Repeat public installer qualification once the split runtime migration is ready.

## 日本語

🟢 [Done] Rustの互換依存3件を更新し、183件のRustテストと既存エンジン切替・プリセット・共有状態・ドッキング検証を通過しました。短い分割パネルで会話一覧が入力欄を押し出す問題も修正しました。Strata 0.2.20候補は新ソースでMetalを再ビルドし、既存Q2_0の認証付き実推論・401拒否・正常停止を確認しました。通常配布のStrata 0.2.15と利用者の設定・会話は保護しています。

🟠 [Next] 0.2.20の上流Macテストは47件中1件が新しいInkling分岐を含む文字列比較で失敗しました。通常版への切替は未実施です。対象の旧設定の自動移行と認証付き応答を確認しました。次に既存パックなしのモデル準備を検証します。⭕️ [Pending] 既存パックなしの準備、別Macでのインストール、OSネイティブのドラッグ操作は未検証です。候補DMGでは隔離環境へのランタイム展開と旧設定・会話の保持、手動切替後の認証付き応答を確認しました。

## 简体中文

🟢 [Done] 已更新三项Rust兼容依赖，通过183项Rust测试及现有引擎切换、预设、共享状态和停靠验证，并修复了较矮分割面板中会话列表挤出输入框的问题。Strata 0.2.20候选版已重新构建Metal，已有Q2_0模型的认证真实推理、401拒绝及正常关闭均通过。保留正式分发的Strata 0.2.15和用户配置、会话。

🟠 [Next] 0.2.20上游Mac测试47项中有1项因包含新Inkling分支的源码字符串比较而失败，尚未切换正式版本。已验证受支持的旧配置自动迁移和认证推理。下一步验证无现成模型包时的完整准备。⭕️ [Pending] 尚未验证无现成模型包时的准备、其他 Mac 安装及原生拖动。已通过候选 DMG 的隔离部署、旧配置与会话保留以及手动切换后的认证推理。
