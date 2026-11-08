# Dependency update · 2026-11-08

## English

🟢 [Done] Reviewed all direct workspace dependencies against npm/crates.io and the fixed MLX environment against PyPI. Updated the compatible application graph: Lucide1.66.0→1.70.0, Vite8.3.5→8.3.6, Playwright1.64.1→1.65.0 (including its two runtime packages), UUID1.29.0→1.29.1 and14 other Cargo packages. Removed three unused defmt packages. Tauri2.12.5, React19.3.1, TypeScript7.0.2 and the remaining direct dependencies are already current. Final npm outdated is empty, npm audit reports zero known advisories, and Cargo's final dry run proposes zero compatible changes. This is not a full security audit. Parent-constrained generic-array/matchit/old TOML generations are not forcibly overridden.

🟢 [Done] Official Strata0.2.9→0.2.12, immutable commit146df3c468739147f520ac3816de806f71232571, source engine0.1.58. Both the gitlink and distribution revision pin move together. An isolated worktree passed the existing local-only reuse contract,42 upstream Mac setup tests, and the focused server/Responses/speed/lifeline suite (433 total:431 passed,2 skipped). Broad serve discovery was interrupted and is not counted as a passing suite. The contract checker now accepts SOLITON_TEST_STRATA_ROOT for isolated candidate checks; its ordinary default is unchanged. Upstream tracked source and requirement pins are unmodified.

🟢 [Done] Bundled Python3.12.15→3.12.16 uses a new versioned install directory; old installed environments and saved user-selected paths remain intact. Bundled Pillow now follows upstream's Python3.11+ pin13.0.0; SentencePiece0.2.1→0.2.3. NumPy2.5.3/regex2026.9.10 and other Strata runtime libraries follow the official upstream requirements, even where PyPI has newer standalone versions. They are not mixed with the independent MLX environment. The bundler uses uv's --no-bin option so it does not change global Python executables. Candidate pip check, isolated imports, relocated imports with a system-only PATH and four Gemma converter tests passed. The distribution Metal engine was rebuilt for macOS15.0; the independent pinned Gemma backend was rebuilt too. A real packaging failure exposed missing build tools in the managed Python; the Metal builder now bootstraps a separate versioned build venv with the immutable upstream CMake/Ninja pins, retaining PEP668 protection and keeping build tools out of the runtime archive.

🟢 [Done] Candidate real Q2_0 inference reused existing local assets: unauthenticated access returned401 and both initial and follow-up questions returned exactly SOLITON114. Ordinary state SHA and model-file metadata were unchanged; owned shutdown exited0. Existing legacy StrataStudio symlinks were left intact; an owned QA link view corrected those paths for the test. No new model weights were downloaded. This small functional smoke is not a performance, broad quality or new-model certification.

🟠 [Next] Qualified MLX runtime migration remains separate. A fresh isolated environment resolved all34 latest package versions and passed pip check, including MLX-LM0.32.2/Transformers5.22.2. The existing retained-KV wrapper correctly refused0.32.2. The cache file is unchanged, but the server has508 diff lines including vision URL processing/cache keys and response-state handling, and tokenizer support also changed. No version/hash allowlist or evidence gate was relaxed. The existing MLX-LM0.32.0/MLX0.32.4/Transformers5.20.2 runtime and its original qualification remain intact. Adoption needs a source-reviewed adapter, full token/answer/cache/lifecycle matrix and original30-minute qualification for the new exact environment. Existing Omni/model/llama pins are independent contracts and are preserved.

⭕️ [Pending] Other Macs/OSes, Developer ID signing/notarization and broad model qualification. Browser fixtures emulate native floating IPC and do not certify OS pointer drag. No universal speedup claim or automatic promotion of an experimental runtime.

🟢 [Done] Final verification:168 Rust tests (162 core +6 Tauri;1 ignored),35 Node tests, frontend build and all-target workspace check; updated Playwright/Chromium engine, split/floating docking and workspace fixtures passed EN/JA/ZH at1440/390/320 widths with no blank screen/framework overlay/relevant console error. Final qualified MLX shared API:36 cases plus3 exact4096 admissions/3 one-token-over refusals; all full answers/input counts match0.1.3. The actual distribution Strata/Python repeated the two-turn Q2_0 smoke successfully. Final Apple Silicon app build passed (96.62MiB); distribution preflight passed all66 native files, fixed revision and relocated Python imports. Fresh isolated native startup provisioned Python3.12.16 and Strata0.2.12, remained offline with empty QA history, and was stopped. Ordinary state/model files remain intact. The initial build failure and its dedicated-build-venv fix are described above; it is not counted as a successful run. QA processes were stopped.

[Machine-readable review](evaluation/dependency-update-20261108.json), [actual Strata inference](evaluation/dependency-strata-real-20261108.json), [MLX shared-API regression](evaluation/dependency-gateway-20261108.json), [distribution preflight](evaluation/dependency-distribution-20261108.json). Browser evidence remains outside the repository: [desktop](/tmp/strata-split-docking-desktop.png), [narrow](/tmp/strata-split-docking-narrow.png). These local screenshots are not portable release artifacts.

## 日本語

🟢 [Done] アプリ依存を公式レジストリと照合し、Vite8.3.6、Lucide1.70.0、Playwright1.65.0、UUID1.29.1ほかRust間接依存へ更新しました。npmの未適用更新・既知脆弱性はゼロ、Cargoも上流制約内で追加更新ゼロです。Tauri・Reactなどの最新安定版は維持し、上流が固定する旧世代の間接依存は強制上書きしません。

🟢 [Done] Strata0.2.12のgitlinkと配布ピンを同期し、Metalを再ビルド。同梱Pythonを3.12.16の別ディレクトリへ更新し、Pillow13.0.0・SentencePiece0.2.3を確認しました。Strataの公式requirementsは変更していません。旧Python・保存済みパス・会話・設定・モデルを保持し、グローバルPythonを変更しない--no-binも追加しました。隔離環境と移設後の読み込み、Gemma変換4テスト、Mac42テスト、重点API431成功/2スキップが通過。実Q2_0の2ターン正答・401拒否・正常終了、MLX共通API36件と三言語境界、Rust168件・Node35件、三言語の画面操作が通過しました。完成アプリは66ネイティブファイルの配布検査と空データ領域での新Python／Strata展開を確認しました。

🟠 [Next] MLX-LM0.32.2などの最新版は別環境で依存解決と互換性調査まで行い、既存保持KVガードの拒否を確認しました。版番号だけの許可追加は行いません。既存の資格確認済み0.32.0環境を保持し、新しいサーバー／トークナイザーの契約確認と完全な30分再資格確認を経て別途採用します。今回それらを最新版へ更新済みとは扱いません。

⭕️ [Pending] 別Mac・他OS、Developer ID署名・公証、広範なモデル品質とネイティブOSドラッグ。実測の範囲を越えた性能向上は主張しません。

## 简体中文

🟢 [Done] 对照官方注册表更新应用依赖：Vite8.3.6、Lucide1.70.0、Playwright1.65.0、UUID1.29.1及14个Rust间接依赖。npm无待更新项或已知漏洞，Cargo在上游约束内无进一步更新。已是最新稳定版的Tauri、React等保持不变，没有强行覆盖上游约束。

🟢 [Done] Strata更新至0.2.12，子模块与分发提交固定值同步，重新构建Metal。内置Python在独立版本目录更新至3.12.16，验证Pillow13.0.0、SentencePiece0.2.3；保留Strata官方依赖固定值和旧Python、普通配置、会话及模型。--no-bin避免修改全局Python。隔离与迁移后的导入、4项Gemma转换测试、42项Mac测试、431项重点API测试（另2项跳过）通过。现有Q2_0两轮回答准确，未认证请求返回401，进程正常停止。MLX共用API36项与三语言边界、168项Rust/35项Node测试和三语言界面交互通过；最终应用66个原生文件的分发检查及空数据目录的新Python/Strata部署验证通过。

🟠 [Next] 最新MLX-LM0.32.2等仅在独立环境完成解析与兼容性调查；现有KV适配器正确拒绝未经审核的版本。保留已认证0.32.0环境，不绕过版本/源码/证据检查。采用新版需先审核服务器和分词器契约，并完成完整30分钟重新认证。

⭕️ [Pending] 其他Mac/操作系统、Developer ID签名与公证、更广泛模型质量与原生系统拖动验证。没有通用加速承诺。

Sources: [Strata0.2.12](https://github.com/Niko1221/Strata/releases/tag/v0.2.12), [Vite8.3.6](https://github.com/vitejs/vite/releases/tag/v8.3.6), [Playwright1.65.0](https://github.com/microsoft/playwright/releases/tag/v1.65.0), [Lucide1.70.0](https://github.com/lucide-icons/lucide/releases/tag/1.70.0), [Tauri update guidance](https://v2.tauri.app/develop/updating-dependencies/).
