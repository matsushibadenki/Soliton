# Dependency update · 2026-10-26

## English

🟢 [Done] Updated official Strata v0.2.4 → v0.2.6 (`4b9e05a2a806a3fe24f7b2560d98626c37dade35`), source engine version 0.1.51. Reviewed the official release and integration changes: empty messages now return 400, one-token speed reporting changed, Rosetta hardware detection improved, and DeepSeek cache defaults changed. Soliton does not enable new model families. Tauri stays on 2.12.5. Cargo refreshed brotli 9.1.0, brotli-decompressor 6.2.0, erased-serde 0.4.11; remaining incompatible Cargo upgrades need their own migration review.

🟢 [Done] Migrated lucide-react 0.468.0 → 1.61.0, TypeScript 5.9.3 → 7.0.2, Vite 7.3.7 → 8.3.5. TypeScript's stricter side-effect imports require `src/vite-env.d.ts`. Vite's build target explicitly retains Chrome/Edge 107, Firefox 104 and Safari 16 rather than silently increasing the target with Vite 8. The Rolldown build warns that Lucide's `use client` directives may not be preserved; this application runs entirely in the client WebView. npm outdated returned an empty object after migration. The floating test now accepts STUDIO_TEST_API for isolated endpoints.

🟢 [Done] Offline upstream compatibility and 33 upstream Mac-setup/answer-speed tests passed. The frontend build and first workspace check / 71 Rust core tests passed. Engine-switch fixtures passed EN/JA/ZH at 390/1440px; menu fixtures passed three locales/four widths; split docking passed EN/JA/ZH, light/dark and 1440/390/320px. Browser plugin skill unavailable: used repository Playwright scripts and installed Chrome. The first menu run lacked SOLITON_TEST_API and tried port 8080; the corrected isolated run passed. Floating validation requires docking_fixture, not the ordinary server; an initial ordinary-server run therefore failed before the correct fixture run.

🟢 [Done] Rebuilt Metal at `Strata/build-metal-v024-make/strata`, metadata digest `293389f8c2b93d7391c3df5877b8549bd3568720af3b717e5c4fc372ce1073ff`. Authenticated loopback inference on port 18956 reused the existing Q2_0 pack/tokenizer and actual GGUF files. A temporary config relocated former StrataStudio paths, bypassed stale source symlinks with the existing actual weight paths, retained 8192 context / prompt chunk 64 / 7 GB reserve, and selected the new executable. No model downloads, file preparation or normal settings changes occurred. Exact answer SOLITON26 passed with reasoning_effort=none: 23 prompt / 7 completion tokens, engine-reported prompt 45200.9 ms / decode 7708.9 ms. This short test ran alongside builds and is not a performance benchmark. The dedicated server stopped with exit code 0. See [successful response](evaluation/soliton-strata-real-20261026.json). The [first attempt](evaluation/soliton-strata-real-20261026-initial.json) exhausted 32 tokens in reasoning and had no final content; it is retained as a failed quality check.

🟠 [Next] Normal saved model configuration still contains former StrataStudio paths and was preserved. Local-file re-preparation and automatic runtime activation were not certified by the temporary inference config. 

⭕️ [Pending] Full upstream server/Responses suites were not repeated; the historical macOS VisionArgs mock-pid limitation remains uncertified. vLLM-Apple real GPU requalification, native WebView interactions, other hardware/models and prolonged stability are outside these fixture/short-inference results.

## 日本語

🟢 [Done] Strata 0.2.6（ソースエンジン0.1.51）、Lucide 1.61.0、TypeScript 7.0.2、Vite 8.3.5へ更新。Tauri 2.12.5を維持し、Rust依存3件も更新しました。CSS型宣言を追加し、Vite更新で対象ブラウザーが狭まらないよう既存対象を明示しました。

🟢 [Done] Metal再ビルド、上流互換性、Mac準備・速度33テスト、71 Rustテスト、3言語エンジン・メニュー・分割ドッキングの回帰テストが成功。既存Q2_0を一時設定で使用した認証付き実推論はSOLITON26に完全一致し、専用プロセスを正常終了しました。思考で上限に達した初回失敗も保存しています。モデルのダウンロードと通常設定の変更はありません。

🟠 [Next] 保存済み設定の旧パス修正・通常起動への適用、ローカル再準備を続けます。

⭕️ [Pending] vLLM-Appleの実GPU再認定、ネイティブ操作と長時間検証は未認定です。

## 简体中文

🟢 [Done] 更新至Strata 0.2.6（源码引擎0.1.51）、Lucide 1.61.0、TypeScript 7.0.2、Vite 8.3.5，保留Tauri 2.12.5并更新三项Rust依赖。补充CSS类型声明，明确保留原浏览器目标。

🟢 [Done] Metal重新构建、上游兼容性、33项Mac准备/速度测试、71项Rust测试、三语言引擎/菜单/分割停靠测试通过。临时配置复用现有Q2_0进行认证真实推理，答案精确匹配SOLITON26并正常停止专用进程。保留首次思考耗尽令牌的失败记录。没有下载模型或修改普通配置。

🟠 [Next] 后续处理保存配置的旧路径迁移、普通启动启用及本地重新准备。

⭕️ [Pending] vLLM-Apple真实GPU重新认证、原生交互及长时间验证尚未认证。

Sources: [Strata release](https://github.com/Niko1221/Strata/releases/tag/v0.2.6), [Vite migration](https://vite.dev/guide/migration).

## Final fixture and lockfile checks

🟢 [Done] After the three Cargo updates, `cargo check --workspace --locked` and all 71 Rust core tests passed again. The dedicated docking_fixture floating suite passed host routing, cross-WebView transport, Cocoa plain-text fallback, split/merge/return, invalid payload rejection and message preservation. It emulates native IPC and does not cover native OS pointer drag. `git diff --check` passed.

日本語：最終lockfileでworkspace確認と71テストを再実行し成功。専用fixtureでフローティング回帰も成功しました。OSネイティブのポインタードラッグは未検証です。

简体中文：最终锁文件的workspace检查和71项测试重新通过；专用fixture浮动停靠测试通过，未覆盖操作系统原生指针拖动。

🟢 [Done] The unsigned Apple Silicon Soliton.app build succeeded again with the final Cargo lockfile (3m37s release compilation, 10.99 MiB bundle). 日本語：最終lockfileで署名なしApple Siliconアプリのビルド成功。简体中文：最终锁文件的未签名Apple Silicon应用构建成功。
