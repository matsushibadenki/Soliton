# Changelog

## 0.1.10 — 2026-11-17

- The existing language-specific retry remains the default. On the exact qualified YES/NO controller and evidence, set `constrain_yes_no_retry: true` with `retry_once: true` and `one_of: ["YES", "NO"]` in `soliton_output_validation` for a non-streaming chat request. After a completed format mismatch, one retry lets the model choose YES or NO and then emit EOS.
- Apply the MLX-LM 0.32.2, internal spans, Gemma 2 prefill 512 and YES/NO constraint patches in that order. Select the rebuilt controller and its matching qualification evidence in Soliton. [Application steps](patches/README.md#qualified-yesno-retry-constraint)
- In 576 fixed-history English, Japanese and Simplified Chinese turns through the shared API, the instruction strategy matched format and content on 286/288 requests and the constraint on 288/288. The gateway reported 148 successful constrained retries. Observed completion medians were 313.5 and 263.1 ms. [Real-model comparison](docs/evaluation/shared-yes-no-gateway-20261117.json)
- Clean reconstruction verified invalid allowlist and old evidence rejection. After a native desktop restart using normal settings, the shared API exercised both retry modes, and all six existing messages matched the backup. [Clean verification](docs/evaluation/shared-yes-no-clean-20261117.json) · [Desktop verification](docs/evaluation/normal-app-yes-no-20261117.json)

日本語：検証済み構成向けのYES／NO再生成オプションを追加しました。既定は言語別指示です。

简体中文：为已通过验证的配置新增 YES／NO 重试选项，默认使用本地化指令。

## 0.1.9 — 2026-11-15

- The shared chat API can validate a single non-streaming response against an exact allowlist and retry once after a completed mismatch. The default retry uses an instruction. Callers can select a shorter localized instruction with `retry_language` (`en`, `ja`, or `zh-CN`).
- Opt in to `constrain_ok_retry: true` with `retry_once: true` and `one_of: ["OK"]` on the exact qualified Gemma 2 controller and its matching evidence. On retry, the backend permits the reviewed `OK` token followed by EOS. A successful response identifies the retry mode in `x-soliton-output-retry`.
- Rebuild the controller by applying the MLX-LM 0.32.2, internal spans, prefill 512, and acknowledgement constraint patches in that order, then select the resulting controller and matching evidence in Soliton. [Application steps](patches/README.md#qualified-singleton-acknowledgement-constraint)
- Across 216 English, Japanese and Simplified Chinese turns, each mode accepted 106/108, recovered 18/18 `OK` retries, and correctly recalled 18/18 updated facts. Median acknowledgement retry time was 351.0 ms with instructions and 249.0 ms with the constraint. The all-turn medians were 184.9 and 205.6 ms. [Shared API measurements](docs/evaluation/shared-ack-option-20261115.json)
- Clean reconstruction verified rejection of older evidence and non-`OK` allowlists. The rebuilt desktop app exercised both retry modes after restart and preserved all six existing messages. [Clean reconstruction](docs/evaluation/clean-ack-constraint-20261115.json) · [Desktop verification](docs/evaluation/normal-app-ack-constraint-20261115.json)

日本語：既定の指示による再生成に加え、資格確認済みの構成で `OK` だけを許す再生成オプションを追加しました。

简体中文：保留默认指令重试，并在已通过资格验证的配置中新增仅允许输出 `OK` 的重试选项。

## 0.1.8 — 2026-11-15

- Added prefill step 512 for the measured Gemma 2 2B IT 4-bit / MLX-LM 0.32.2 profile at 4,096 tokens, concurrency one, 1,536 MiB retained KV and a 512 MiB allocator cache.
- In 24 shared API requests within the CPU load criteria, full answers and token counts matched. Median first output fell from 7.009 s to 6.755 s and median response completion from 7.222 s to 6.955 s. Sampled peak physical footprint fell by 87 MiB. [Comparison](docs/evaluation/shared-prefill-quiet-20261115.json)
- The integrated runtime passed 1,800.243 s of qualification with 13,482 successes and zero failures. All 33 mixed KV requests passed. A clean reconstruction passed six multilingual requests, and the native desktop restart preserved all six existing messages. [Qualification](docs/evaluation/integrated-prefill512-20261115.json) · [Clean reconstruction](docs/evaluation/clean-prefill512-20261115.json) · [Desktop verification](docs/evaluation/normal-app-prefill512-20261115.json)
- Apply the MLX-LM 0.32.2, internal spans and prefill 512 patches in order, then select the controller and its matching qualification evidence in Soliton. [Application steps](patches/README.md#rebuild-from-the-controller-base)

日本語：実測したGemma 2構成に512を組み込みました。30分の再資格確認、共有API、通常のデスクトップ再起動での履歴保持を確認しました。

简体中文：在经实测的 Gemma 2 配置中集成了512，并通过30分钟重新认证、共享 API 验证和桌面重启后的历史保留检查。

## 0.1.7 — 2026-11-12

- SSD cache restore copies and validates the payload in one pass. Median CLI restore time fell from 48.25 ms to 33.69 ms in the matched synthetic workload using optimized builds.
- Added bounded persistence diagnostics, verified publication of qualification evidence, and cleanup of interrupted restore files while the engine is stopped.
- The optional Gemma2 SSD profile passed 1,800.509 seconds with 6,582 successes, zero failures and 784 KiB sampled peak RSS growth. All 18 Rust-managed checks passed. [Scope and measurements](docs/SSD_PREFIX_CACHE.md#single-pass-ssd-restore).

## 0.1.6 — 2026-11-10

- Added optional Rust launcher preparation/process-spawn wall times across all five launch paths while preserving readiness timing origins.
- Added an isolated, exactly source-guarded MLX-LM0.32.2 internal-span profile: full startup evidence/model/backend verification, RAM LRU fetch/insert/trim, batch prompt/decode and sequential generation iterator timing. Fixed scalar ledgers retain no prompts/tensors; Rust validates schema/values and binds diagnostics to live settings/session with existing byte/time/expiry limits.
- Original full qualification passed1800.111s,14588/14588 requests, zero failures,128KiB peak RSS growth and clean shutdown. Only controller runtime identity changed; installed backend/model/environment/hardware identities and ordinary settings are preserved.
- Old/candidate24+24 multilingual JSON/SSE/seeded/batch parity and shared API36 cases/4096–4097 boundaries match complete answers and usage. Pressure/eviction/reheat, old-evidence refusal/recovery, cancellation/switch/restart and chat persistence pass.
- 166 Rust,4 span,5 existing0322 contract,75 controller and5 gateway-tool tests pass; frontend/server builds, Tauri check and seven-file patch replay pass. Failed QA setup attempts remain recorded, with no guard relaxed.
- Source release with an opt-in measured profile; no GPU-time or speedup claim. SSD persistence/CLI spans, other Macs/models, native GUI for this profile and signed/notarized installers remain separate. [Evidence and reproduction](docs/ENGINE_INTERNAL_SPANS.md).

## 0.1.5 — 2026-11-08

- Added a separate MLX-LM0.32.2 plain-text profile: exact release/server/cache binding, pre-URL text validation and complete installed-file/architecture evidence guards. Existing0.32.0 runtime/evidence and ordinary settings remain intact.
- Qualified the fixed4096/concurrency1/KV1536MiB/allocator512MiB profile using the original30-minute runner:1800.063s,11871 successes,zero failures,327680-byte peak RSS growth, matching identity and clean shutdown. No global compatibility-matrix promotion.
- Added only the measured Transformers5.22.2 tuple to the exact Gemma CPU counter after1091 full-ID comparisons and six fallback guards; previous tuples remain.
- Shared API36 cases and three-language boundaries preserve all full answers/input/output/cache counts/finish reasons versus0.1.4. Eviction/reheat, old-evidence refusal/recovery, cancellation, owned cleanup, restart and chat SSE/persistence pass.
- Source release; native GUI for the new profile, other Macs/models and signed distribution remain pending. [Full scope, preserved failed harness attempts and reproduction](docs/MLX_0322_QUALIFICATION.md).

## 0.1.4 — 2026-11-08

- Updated Vite8.3.6, Lucide1.70.0, Playwright1.65.0, UUID1.29.1 and14 other compatible Cargo dependencies; latest stable Tauri/React retained.
- Updated the Strata gitlink and distribution pin to official0.2.12, with a rebuilt Metal engine and bundled Python3.12.16/Pillow13.0.0/SentencePiece0.2.3. Existing installed Python and user-selected settings are preserved.
- Isolated CMake/Ninja build tools in a dedicated venv after a real packaging test exposed PEP668 protection in the new Python. Runtime archives exclude build tools; no global Python executable changes.
- Qualified MLX/Omni runtimes remain pinned. A latest-MLX candidate was investigated in isolation and correctly refused by the existing source guard; its adoption requires separate source review and full requalification.
- See [dependency scope and verification](docs/DEPENDENCY_UPDATE_20261108.md). Source release; signed/notarized installers and other-Mac validation remain pending.

## 0.1.3 — 2026-11-08

- Moved qualified MLX CPU-tokenizer preparation from state reads to a Rust-owned startup monitor, covering API-only clients without UI polling. No inference or token-count frames are sent during preparation.
- Added terminal preparation status and tests for stale health, stopped sessions, failed preparation and server-owned cleanup. Qualification checks and request admission remain enforced.
- Four-start before/after/after/before real-model comparison with six seconds idle: first admission median297.57→4.98ms; first-content median682.88→428.38ms. Answers and token/cache counts match; initialization moves earlier, with no GPU speedup claim.
- Source release; ordinary settings remain unchanged. See [measurement scope](docs/INFERENCE.md#autonomous-startup-prewarm--2026-11-08).

## 0.1.2 — 2026-11-08

- Added a reproducible symmetric retained-KV pilot with original candidate preflight, complete identity/count/answer checks, CPU eligibility and native owned-tree memory samples.
- Qualified an optional1536MiB retained-KV profile for the fixed4096-token MLX runtime:1800.123s,12167 successes,zero failures. Shared API36 cases, boundaries, five-history pressure/reheat and mismatched-evidence refusal/recovery passed.
- In the scoped direct pilot, long-history full reuse changed0/12→12/12 with about476MiB additional sampled footprint. Short/decode tradeoffs and the ineligible2GiB timing window remain recorded. Ordinary/default settings are unchanged.
- This remains a source release; signed/native cross-Mac distribution is pending. See docs/INFERENCE.md and docs/ROADMAP.md for exact scope.

## 0.1.1 — 2026-11-08

- Added correlated Rust gateway wall times and per-start engine session IDs, preserving legacy timing fields.
- Added durable volume-bound v2 qualification for opt-in MLX SSD; legacy certificates remain refused.
- Added cache comparison preflight, balanced ABBA/BAAB ordering and explicit rejected-run evidence.
- Added a fixed, isolated PyPI MLX-LM0.32.0 runtime snapshot and reviewed source-pair patch for the 4,096-token path.
- Extended the measured lightweight tokenizer selector to Transformers5.20.2 while preserving5.19.0; full token-ID parity and shared-API regression passed.
- Added a multilingual shared-API baseline tool. Real results and qualification scope are linked from `docs/ROADMAP.md`.

This is a source release. Signed/notarized native installers and other-Mac validation remain pending. No ordinary model/settings selection is changed by installing the source tree.

日本語：検証が完了した区切りでパッチ版を0.0.1ずつ更新します。速度比較・機能検証・資格証跡の範囲を分けて記録します。

简体中文：每完成一个验证里程碑，将补丁版本增加0.0.1；速度比较、功能验证与资格记录分别注明适用范围。
