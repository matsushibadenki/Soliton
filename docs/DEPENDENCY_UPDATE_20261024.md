# Dependency update · 2026-10-24

## English

🟢 [Done] Strata source moved to stable v0.2.4 (`96ca4383c7b05a8a3e6064c65d9e9977d4b3bb59`). Tauri Rust 2.12.3 → 2.12.5, tauri-build 2.7.2 → 2.7.4, dialog 2.7.2 → 2.8.3, npm CLI 2.12.3 → 2.12.4; API remains 2.12.4. Cargo updated 49 compatible packages; npm updated five installed packages and refreshed platform lock entries. Existing user changes were retained.

🟢 [Done] Frontend build, offline upstream contract, workspace check, 69 Rust core tests, isolated engine-switch browser suite (EN/JA/ZH, 390/1440px), and unsigned Apple Silicon application build passed. The new Metal engine built at `Strata/build-metal-v024-make/strata` using existing metal-cpp/llama.cpp. The old build cache refers to the former StrataStudio directory; it and its executable were retained. Ninja was unavailable, so the new build uses Unix Makefiles. New model families are not enabled by this update.

🟠 [Next] Review separate major upgrades: lucide-react 0.468.0 → 1.61.0, TypeScript 5.9.3 → 7.0.2, Vite 7.3.7 → 8.3.5. They were reported by npm outdated after the compatible update and have not been adopted. Review remaining Cargo major upgrades separately.

⭕️ [Pending] Real Strata inference and local-file re-preparation with the new executable. Existing strata-q2_0.json contains absolute paths to the former StrataStudio folder; do not treat prior inference evidence as certification of v0.2.4. Runtime configuration and the old engine binary were not replaced. Real vLLM-Apple GPU qualification and native interactive docking were not repeated; fixture tests do not certify these.

## 日本語

🟢 [Done] Strataを安定版0.2.4へ、Tauri本体を2.12.5へ更新。互換範囲内でRust/npmのlockfileを更新し、既存の作業変更を保持しました。フロントエンド、互換性契約、workspace、Rust 69テスト、3言語のエンジン切替ブラウザーテスト、署名なしApple Siliconアプリのビルドが成功しました。Metalは旧キャッシュを保持して `Strata/build-metal-v024-make/strata` に新規ビルドしました。

🟠 [Next] lucide-react、TypeScript、Viteのメジャー更新は個別に移行・検証します。

⭕️ [Pending] 既存モデル設定に旧StrataStudioフォルダの絶対パスが残っているため、新Metalでの実推論とローカル再準備は未検証です。設定と旧実行ファイルは変更していません。新モデル対応、vLLM-AppleのGPU再認定、ネイティブドッキングの操作検証も未認定です。

## 简体中文

🟢 [Done] Strata已更新至稳定版0.2.4，Tauri已更新至2.12.5。更新了兼容范围内的Rust/npm锁文件，并保留现有工作修改。前端构建、上游兼容性契约、workspace检查、69项Rust测试、三语言引擎切换浏览器测试及未签名Apple Silicon应用构建均通过。保留旧构建缓存，新Metal引擎位于 `Strata/build-metal-v024-make/strata`。

🟠 [Next] 分别评估并验证lucide-react、TypeScript及Vite的大版本升级。

⭕️ [Pending] 现有模型配置仍引用旧StrataStudio目录，尚未验证新Metal引擎的真实推理及本地重新准备。未替换运行配置或旧引擎。新模型、vLLM-Apple GPU重新认证及原生停靠交互尚未认证。

Sources: [Strata v0.2.4](https://github.com/Niko1221/Strata/releases/tag/v0.2.4), [Tauri 2.12.5](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.5).

## Upstream API regression result

The existing Strata Python environment ran `python -m unittest serve.test_server serve.test_responses serve.test_dsv41`: 453 tests in 160.463 seconds, 450 passed, 2 skipped, 1 error. `serve.test_server.VisionArgs.test_min_tokens_is_passed_only_when_set` fails because its fake `Proc` lacks `pid`, which the macOS lifeline watcher requires. This matches the limitation recorded in the 2026-10-21 compatibility report. Upstream sources were not patched; vision is not certified. System Python initially lacked jinja2/regex, so the complete run used Strata/.venv/bin/python. Playwright initially lacked its newly versioned bundled browser; the successful fixture run used installed Google Chrome via PLAYWRIGHT_CHROMIUM_EXECUTABLE.

日本語：上流453テスト中450成功、2スキップ、1エラー。既知のVisionArgsテスト用Procにpidがなく、macOS監視処理で失敗します。上流ソースは変更していません。

简体中文：上游453项测试中450项通过、2项跳过、1项错误。已知VisionArgs模拟Proc缺少pid，导致macOS监视器失败。未修改上游源码。

The rebuilt Metal metadata reports engine source version 0.1.49 and digest `60c708cf74d6e747881bc41d1c7810496a61f3cda4a9f43302243feae6f73966`. No model was downloaded. `git diff --check` passed.


## Later vLLM-Apple environment follow-up

The current M4/Gemma shared-Rust GPU path has since been requalified with the added login flag `PYTORCH_ENABLE_MPS_FALLBACK=1`: full 30-minute qualification and the long-history cache/lifecycle workload passed. Only evidence preferences changed; vLLM model/backend/runtime identity remained unchanged. See [results and scope](BENCHMARKS.md#diverse-long-history-cache-memory--2026-10-24). Strata real-engine preparation/inference items above remain pending.

日本語: その後、現在の環境変数を含むvLLM-Apple実GPU再認証と長文キャッシュ試験は合格しました。上記のStrata実モデル再準備・推論は引き続き保留です。

简体中文：随后，包含当前环境变量的vLLM-Apple真实GPU再认证及长历史缓存测试通过。上述Strata真实模型准备／推理仍待验证。
