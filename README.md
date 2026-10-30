# Soliton

![Soliton](docs/images/stratastudio-image.png)

[日本語](README.ja.md) · [简体中文](README.zh-CN.md)

A Tauri 2 inference workbench and shared Rust API gateway for Strata and vLLM-Apple on Apple Silicon. Rust owns conversations, history, logs and metrics across all windows. Engines run separately and are selected manually.

## Engine selection

Preferences → Engine → Inference engine selects **Strata** or **vLLM-Apple**. Switching stops the previous managed engine and its workers, then saves the selection; click Start to load the selected engine. Active GUI/API requests and model preparation block switching. Only one managed engine runs at a time. Independently started processes are outside Soliton's lifecycle; stop those before using Soliton.

For vLLM-Apple, set its source directory (`vLLM-apple/` beside `Strata/`), absolute Python path, existing absolute local model directory, backend (`MLX LM` or `vLLM-Metal`) and absolute backend executable path. Use a compatible environment already prepared by vLLM-Apple. The packaged Strata runtime includes Python and dependencies; vLLM-Apple keeps its separate environment. Local models can download files from a Hugging Face URL, but engine compatibility is checked separately. Moving a virtual environment may leave executable shebangs pointing to its previous directory; repair or recreate the selected environment when necessary. MLX uses model alias `default_model`; vLLM-Metal uses the configured model path. The daemon's `inference_ready` controls chat availability.

Chat, saved history, logs, process-memory history and request metrics are shared. Each new request records its engine; older saved requests show an unavailable engine value. Switching keeps the conversation, but the next engine receives it as a new prompt: KV caches are not transferred. Backend-specific context, role and tool support still apply. OpenAI routes are proxied; Anthropic `/v1/messages` is advertised only for Strata and is not translated for vLLM-Apple.

The desktop product is now **Soliton**, bundle ID `dev.soliton.desktop`. When its new state file is absent, it reads `dev.strata.studio/studio.json` from the previous sibling application-data directory, and future saves use the new directory. The original file is preserved. The standalone server retains `.studio-state.json` and existing `STUDIO_*` variables for compatibility. The workspace folder and engine names remain unchanged.

Run `npm run test:engines` after `npm run build` and `cargo build -p studio-server` for isolated protocol-fixture browser tests on port 18080 (no weights/GPU). Set `PLAYWRIGHT_CHROMIUM_EXECUTABLE` if using an installed browser. Real model quality, memory peaks and long-run performance require separate validation.

The local `vLLM-apple/` checkout remains an independent Git repository and is ignored by the parent repository. On another machine, clone it separately with `git clone https://github.com/matsushibadenki/vLLM-apple.git vLLM-apple`, then prepare its supported environment. Source changes and existing evaluation evidence stay under that repository's own version control.

## Run

```sh
npm ci
npm run desktop
```

The desktop app owns its loopback API server on `127.0.0.1:8080`. Do not run the standalone server at the same time.

For a browser workbench or a headless API gateway:

```sh
npm run build
cargo run -p studio-server --release
```

Open `http://127.0.0.1:8080`. For frontend development, run `npm run server` and `npm run dev` in separate terminals, then open `http://127.0.0.1:5180`.

## Connect Strata

Open **Soliton → Preferences… → Engine**. Set the Strata directory, an absolute Python 3.10+ interpreter path with Strata's dependencies, and a `strata-*.json` configuration produced by upstream setup. Start the engine. The `.venv/bin/python` interpreter and existing configurations are detected on first launch.

For API testing without model files, select **Test engine**. This uses the upstream mock engine; its outputs and logged speeds are not real inference measurements.

Real inference requires the upstream Metal executable, prepared model pack, and tokenizer. Follow [Strata's Mac guide](Strata/docs/MAC.md) to prepare these. Use the Preferences → Engine’s **Prepare model** icon to check paths/Python, select an existing config, or prepare one of the six supported Metal models. The preparation dialog shows model download and working-space estimates. A new download starts only after checking the capacity consent box and pressing **Prepare model**. Setup runs in a dedicated Strata `.venv`, streams its logs, can be stopped, and applies the generated config/Python path on success. It keeps the inference engine stopped. Studio does not download a model automatically. On the development Mac (32 GiB), upstream uses SSD expert streaming and keeps its memory protection defaults.

On Apple Silicon, Studio automatically caps context and prompt buffers by physical memory, reapplies them after model preparation and before startup, and preserves existing smaller limits. The 32 GiB profile uses 8K context and chunks of 64. See [Mac memory policy](docs/MAC_MEMORY.md). Preferences → Engine now shows automatic/active memory values and supports Manual context, prompt batch and macOS reserve settings. Use Apply & restart to save and restart in one action, including while running; plain Apply saves while stopped. Values survive re-preparation. The Engine tab also shows live system-wide memory, available memory and a before-change comparison; these include reclaimable cache and other apps, not only Strata. Memory history charts compare Mac-wide usage and the Strata process over 2, 5 or 10 minutes, with settings, start/stop and inference markers. Rust keeps recording while chatting; history resets on app exit.

The logo opens About, Preferences and Quit. File/Edit/View/Help menus provide conversation export, text editing and workspace switching. Preferences has **General** (language), **Engine**, and **API** tabs. Switch the main workspace using the **Chat / Logs** tabs at the bottom. `⌘,` opens Preferences; `⌘N` starts a new conversation; `⌘1` / `⌘2` switch the workspace. File → Export conversation saves JSON through a native save dialog (browser builds download JSON).

Configuration, conversation, and in-flight generated text belong to Rust. Configuration, conversation, workspace tab order and selection survive restart; the unsent composer is temporary interaction state. Path fields retain their native file/folder pickers.

## Local model discovery

Open **Preferences → Engine → Local models** (folder/search icon), also available from **Prepare model**. The library scans automatically when opened; Refresh scans again. Add a custom folder using the path field or the desktop folder picker. Removing a search folder only removes the location from Studio settings.

| Source | Locations inspected |
| --- | --- |
| Strata | Source tree, sibling `Strata-data`, upstream `data_dir`, and model/pack paths in the selected run config |
| LM Studio | `~/.lmstudio/models`, legacy `~/.cache/lm-studio/models`, `downloadsFolder` / `modelsDirectory` from its settings |
| Unsloth Studio | Studio HF cache, exports/models, `~/.unsloth/.cache`, shared HF cache, stored HF cache home/history and cache environment overrides |
| Ollama | `~/.ollama/models`, `OLLAMA_MODELS`; the standard Linux service location on Linux |
| Custom | Up to 32 saved absolute paths or `~/…` paths, including mounted volumes |

The Rust inventory reads filesystem metadata and a 24-byte model header, follows model-cache symlinks with cycle protection, and deduplicates physical files. Ollama manifests supply names for extensionless GGUF blobs. SafeTensors/PyTorch and Strata expert-pack files are listed separately. Scanning alone does not establish inference readiness. Select a row to check and use it as described below.

Scans run on the Rust blocking pool and cache results for 30 seconds. Traversal is bounded to 100,000 entries, 2,000 model files, depth 16 and a 15-second traversal budget; incomplete scans and inaccessible folders are shown. Narrow the custom search path when a limit is reached. The authenticated `GET /studio/models` endpoint returns metadata; `?refresh=true` bypasses the cache. `npm run test:models` verifies folder persistence, filtering, classification, three languages and responsive layouts; Rust fixtures also cover all providers, location overrides, Ollama naming, symlinks/hardlinks, cycles and old settings.

### Select and reuse a model

Select a model in the library, then press **Use in Strata**. If a usable Strata configuration already references that file or pack, Studio activates it immediately. Otherwise the current Metal preparation path accepts complete Qwen3.8-Flash-Next GSQ-RCO **Q2_0 / IQ3_XXS / IQ3_S** pairs and Unsloth **UD-Q4_K_XL / UD-Q5_K_XL / UD-Q6_K_XL** shard sets with upstream filenames. The selected logical cache path is retained so HF snapshot filenames are available even when their targets are hash-named blobs. Other models/formats require their own supported runtime/configuration and are refused by this preparation path.

Before preparation, a bounded header inspection uses Strata's GGUF reader to check `qwen4exp`, split indexes/counts, duplicate/incomplete tensor sets and tensor bounds. Missing, inconsistent or truncated shards stop before setup. The reuse job creates symlinks in `Strata-data/local-models/<model>-<fingerprint>/sources`; original model files and their completion markers are untouched. It calls upstream Metal setup with `--gguf-dir` through an in-memory adapter that treats the validated source links as complete and blocks weight downloads, isolates generated artifacts from upstream's file migration, and disables optional MTP checkpoint fetching. Model weights are not re-downloaded; tool/source archives, Python dependencies, engine compilation and derived pack/repack output may still be required. Workspace estimates exclude the existing model download size.

The supervised job supports logs/cancel, verifies the generated engine/tokenizer config and applies it on success. The engine stays stopped until **Start**. Failure retains the previous active settings and never falls back to downloading the model. `npm run test:reuse` checks real API selection, missing-shard refusal, config activation, supervised fixture preparation with zero model downloads, localization and narrow selection layouts. Rust tests also verify unchanged source bytes/sidecars and the download guard. Real large-model build/inference remains pending on this development machine.

Location references: [LM Studio](https://lmstudio.ai/docs/app/advanced/import-model), [Ollama](https://docs.ollama.com/faq#where-are-models-stored), [Unsloth storage roots](https://github.com/unslothai/unsloth/blob/main/studio/backend/utils/paths/storage_roots.py) and [Unsloth HF cache settings](https://github.com/unslothai/unsloth/blob/main/studio/backend/utils/hf_cache_settings.py).

## API

Copy the **Session API key** from **Preferences → API**. The key rotates when Studio restarts and is not saved. Use it as `Authorization: Bearer`, `x-api-key`, or `x-studio-token`.

| Route                       | Behavior                                                                        |
| --------------------------- | ------------------------------------------------------------------------------- |
| `GET /v1/models`            | Loaded model list                                                               |
| `POST /v1/chat/completions` | OpenAI-compatible JSON or SSE                                                   |
| `POST /v1/messages`         | Anthropic-compatible JSON or SSE                                                |
| `POST /v1/responses`        | Upstream Responses API passthrough                                              |
| `GET /status`               | Upstream inference state                                                        |
| `GET /health`               | Gateway health; does not imply the model is ready                               |
| `GET /studio/inspect`       | Authenticated path/config/Python diagnostics and Metal model capacity estimates |
| `GET /studio/state`         | Authenticated shared application state, engine health, bounded logs             |

Only loopback listeners are supported. The upstream Python server uses a separate private port and key. Responses stream through Rust without buffering the complete answer. Model weights, frame data, and GPU buffers never enter a WebView.

The standalone gateway accepts `STRATA_ROOT`, `STUDIO_PORT` (default `8080`), and `STUDIO_STATE_PATH` (default `.studio-state.json` in the working directory). The desktop app saves settings and conversation to its application data directory as `studio.json`. Saved state contains conversation text; remove that file to reset it.

## Build and verify

```sh
npm run build
cargo check --workspace --locked
cargo test -p studio-core --locked
npm run desktop:build -- --bundles app --no-sign
```

Production builds use Thin LTO, one codegen unit, and stripped symbols; the desktop build command targets `aarch64-apple-darwin` and macOS 15+. Signing/notarization requires the distributor's Apple credentials.

For end-to-end tests, keep the Rust server and Vite running, install the Playwright Chromium runtime (`npx playwright install chromium`), then run `npm run test:e2e`. `STUDIO_TEST_PYTHON` selects the test interpreter; `PLAYWRIGHT_CHROMIUM_EXECUTABLE` can select an existing Chromium installation. Use an isolated `STUDIO_STATE_PATH` for test runs and keep the desktop app closed. `npm run test:roadmap` additionally checks preparation diagnostics, config discovery/selection, consent enforcement without downloading, tabbed preferences and workspace navigation. `npm run test:menus` checks menus, About, editing, export, shortcuts, three locales and responsive preferences. Rust tests use a fake setup script to verify completion, failure, cancellation and environment isolation without model downloads. The base test covers lifecycle, streamed GUI chat, API compatibility/authentication, three languages, and 320/375/414/768/1440px widths.

See [architecture](docs/ARCHITECTURE.md) and [roadmap](docs/ROADMAP.md).

Conversations are saved automatically. Use the history icon to search and resume them; New Chat preserves the previous conversation. Deletion requires confirmation, and File exports the selected conversation as JSON.

Preferences → General lets you change conversation/composer text size (12–32 px) and switch between Light and Dark. These preferences are saved across restarts.

Expand reasoning/tool-call sections and the metrics row below a response to inspect model output and per-request latency, reported usage and engine timings. Preferences → API lists the latest 100 finished requests (including cancellations and failures). Missing values stay unavailable; see [inference metrics](docs/INFERENCE.md).

Workspace tabs support drag reordering, arrow buttons and Alt+Left/Right. Float the Chat/Logs group with the outward-arrow icon; dock it with the return icon or close the floating window. Both windows share Rust state. See [workspace controls](docs/WORKSPACE.md).

The dock-layout icon splits Chat and Logs left/right or above/below, or merges them back into one tab group. Drag tabs onto edge/center targets for the same actions, resize the boundary, and float either group independently. Split geometry survives restart; narrow windows stack the groups. Floating tabs can dock directly into the main workspace or another floating window. Drop onto an edge to split or the center to merge; the destination stays open and empty source windows close automatically.

## Inference measurements

With an existing model configured, run `npm run benchmark:inference -- --start --output /tmp/strata-benchmark.json`. This opt-in loopback test uses synthetic retrieval prompts of increasing length, records engine-reported tokens/timings and sampled Mac/engine memory, and stops an engine it started. It does not download models or change conversations/settings. See [measurement procedure and limits](docs/BENCHMARKS.md).

Validated against the Strata 0.2.1 line at `9de04cd4` (source version 0.1.46.3). See [upstream compatibility and verification](docs/UPSTREAM_COMPATIBILITY.md).

[vLLM-Apple qualification and real-model testing](docs/VLLM_APPLE.md)。

Verified local Gemma 2 2B inference is available through the existing identity-bound evidence gate; see the linked procedure for the exact environment and seven-day renewal.

Current Strata source: v0.2.7.1; Tauri 2.12.5. See [dependency update and validation limits](docs/DEPENDENCY_UPDATE_20261026.md), including authenticated inference with a temporary config and remaining saved-path migration.

Latest update: [2026-10-26 dependency migration and real-inference limits](docs/DEPENDENCY_UPDATE_20261026.md).

Latest dependency check: [2026-10-29 versions, compatibility fixes and qualification limits](docs/DEPENDENCY_UPDATE_20261029.md).

## Gemma 3 text with MLX

The vLLM-Apple engine recognizes `gemma3_text`. `google/gemma-3-1b-it` was qualified on the development Mac with MLX-LM 0.32.0, context 512 and concurrency 1. In Preferences → Engine, select vLLM-Apple / MLX LM, the downloaded local model folder and the matching owner-only qualification JSON; run diagnostics and Start. Set the output limit to 64 in Preferences → API for the demonstrated short chat. Other Macs/builds need their own evidence. See [deployment and verification](docs/DEPLOYMENT.ja.md).


## Gemma 3 1B in bundled Strata

On the Apple Silicon distribution, select the downloaded `google/gemma-3-1b-it/model.safetensors` in Local models and choose “Use in Strata”. Bundled Python converts it to F16 GGUF; the Strata selection then runs a dedicated bundled llama.cpp Metal backend. MLX, system Python and developer tools are unnecessary. Source weights remain unchanged; allow at least 2.5 GB of free conversion space. This path supports the Gemma 3 1B text layout, context 2048 and one slot. It is separate from Strata's Qwen kernels; vision and other Gemma sizes are outside this scope. See [deployment and validation](docs/DEPLOYMENT.ja.md).


Run `npm run check:distribution -- --smoke` for local preflight and `npm run package:validation` for a checked ZIP. See the [trilingual clean-Mac checklist](docs/CLEAN_MAC_VALIDATION.md). Verification on another Mac remains pending.
