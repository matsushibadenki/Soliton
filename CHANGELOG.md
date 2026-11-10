# Changelog

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
