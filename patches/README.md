# Local vLLM-Apple patches

`vLLM-apple/` is an independently managed, ignored checkout. Keep its changes reviewable and reproducible here rather than silently depending on an untracked local edit.

Patch base: `90f9c2bbd79c24d6d1f82122366e86345268b8b5`.

`vllm-apple-fingerprint.patch` changes only fingerprint path resolution/containment and regression tests. It retains complete-byte hashing, isolated probes, before/after file checks, inventory limits/recheck and rejection of external paths/symlinks. macOS uses the system strict `realpath` implementation; other platforms retain pathlib. Native canonical spelling can differ for case-aliased paths, so unchanged installation digests are compared explicitly for the measured environment. This is not a persistent cache or approval bypass.

Apply only to a compatible clean checkout with `git -C vLLM-apple apply --check ../patches/vllm-apple-fingerprint.patch`, then `git -C vLLM-apple apply ../patches/vllm-apple-fingerprint.patch`. If the check fails, reconcile the actual source rather than forcing application. Runtime source changes invalidate existing qualification evidence and require a fresh full qualification before inference. The frozen pre-change fingerprint worker in `scripts/fixtures/fingerprint-reference.py` is for differential measurements only; it is not used by Soliton at runtime.

日本語: vLLM-Appleは独立した管理対象のため、変更をパッチとして保持します。適用前に`apply --check`し、不一致を強制適用しません。全バイト検査・変更検出・上限・外部パス拒否は維持します。runtime変更後は新しい完全な検証証跡が必要です。

简体中文：vLLM-Apple为独立管理的检出目录，修改保留为补丁。应用前执行`apply --check`，不强制覆盖冲突。完整字节检查、变更检测、限制及外部路径拒绝保持启用。runtime修改后必须取得新的完整验证证据。

## Retained KV budget · 2026-10-25

`vllm-apple-kv-budget.patch` applies to local checkout base `73874ccd492f55db402d60835a5e5577663c7751`. It adds the opt-in `VLLM_APPLE_KV_CACHE_MIB` policy, backend-adjacent isolated launcher, reviewed MLX-LM 0.32.0 server/cache source guards, retained-LRU byte enforcement for both seeded sequential and batch serving, and read-only scalar KV/allocator snapshots. It does not edit the installed engine, alter attention, truncate contexts or disable startup verification. The normal environment/runtime evidence binding covers the policy and wrapper. Requalify in full before use; an old report cannot approve this changed runtime/policy.

The patch was check-applied to a temporary clean-base fixture, applied, and all three resulting files matched the working checkout byte-for-byte. Source guards target installed `server.py` SHA-256 `8514178d18ee7e5edd1db8b8077ab97079ec72d1db666b85c110fb33cdc55147` and `models/cache.py` SHA-256 `440709018cc528ee1e4e42e61ff8713ed2e0079566d9e8fa58eed3a92d334404`.


## Allocator free-cache budget · 2026-10-25

`vllm-apple-allocator-budget.patch` applies **after** `vllm-apple-kv-budget.patch` on base `73874ccd492f55db402d60835a5e5577663c7751`. It adds opt-in `VLLM_APPLE_ALLOCATOR_CACHE_MIB` (64–4,096 MiB, only with the reviewed KV launcher). The source-guarded wrapper calls the installed MLX `set_cache_limit` once before model load and keeps a stricter existing limit, without per-request flushing or an active-memory cap. Scalar metrics separate configured/effective free-cache limits from active/cache/peak allocation values. Full runtime/environment evidence binding remains mandatory.

Check/apply this patch to a temporary KV-patched baseline first. All three resulting files were compared byte-for-byte with the local independent checkout. Existing installation files are unchanged; changing the wrapper or either budget requires fresh full qualification.

日本語: KVパッチの後に適用します。再利用用の空きGPUメモリだけに上限を設定し、使用中の重みやKVは解放しません。元の完全な検査と新しい証跡が必要です。

简体中文：在KV补丁之后应用。仅限制可复用空闲GPU内存，不释放使用中的权重或KV；仍需完整检查及新证据。


## PyPI MLX-LM0.32.0 source pair — 2026-11-08

`vllm-apple-mlx-0320-pypi.patch` applies after the existing KV/allocator patches. It preserves the legacy pair and admits only the exact reviewed PyPI server/cache pair57392576…27f79 /c96cb169…45e6. Mixed revisions and unknown hashes remain refused. Real LRUPromptCache insert/trim/fetch signatures, byte accounting and max-bytes enforcement were reviewed; the new cache-type accounting does not change the wrapper's lock or byte limit. `scripts/verify-kv-lru.py` tests the exact server/cache pair and real LRU with logical-byte fixtures; no weights are needed. Two source-pair tests reject crossed pairs and mutated hashes. New runtime/source identity still requires full fresh qualification.

The old Homebrew environment currently contains development MLX-LM0.32.3.dev12 and external RECORD links, correctly refused by the full inventory check. Do not remove that guard or modify the global installation. Use a private Python3.12.16 venv and `uv pip sync --python /absolute/private/python scripts/fixtures/mlx-0320-runtime-20261108.txt`; the34-package snapshot pins MLX-LM0.32.0 /MLX0.32.4 and related dependencies. Local model weights are reused; no model download or remote code is required. A new venv/path/content needs its own original qualification before ordinary startup. Package pins do not replace byte fingerprinting.

日本語: グローバル環境は変更せず、固定した専用環境で全ファイル照合と再資格確認を行います。新旧のserver/cacheを混在させる組合せは拒否します。

简体中文：保留全局环境，使用固定的独立环境完成全部文件校验和重新资格验证；拒绝混用新旧server/cache。

## MLX-LM0.32.2 plain-text profile — 2026-11-08

`vllm-apple-mlx-0322.patch` targets clean controller commit `bdef9567191bcc46154c0f7cf7f05089b718f0b4`. It is a combined alternative to `vllm-apple-mlx-0320-pypi.patch`, not a patch to force over an already modified checkout. `scripts/mlx-retained-kv.py` is the canonical resulting wrapper. Check/apply to a separate clean controller copy; compare the resulting file byte-for-byte with the canonical script. The patch was check-applied/applied to a temporary original-source fixture and matched exactly. Keep any existing qualified controller/environment intact.

The allowlist now binds **release and exact server/cache pair together**: the two previously reviewed0.32.0 pairs remain, and0.32.2 admits only server8d475058…0723/cachec96cb169…45e6. Unknown/development releases, changed files and incorrect version/source combinations remain refused. The0.32.2 cache and Gemma2 implementation are byte-identical to the selected PyPI0.32.0 versions. Generation/tokenizer/model-loading and response-state differences were reviewed; full installed-file inventory and architecture evidence still bind all files, not only these two hashes.

The0.32.2 server can inline image URLs before model-specific processing. This text profile replaces that exact guarded function with plain-string role/content validation before URL inlining, inside the upstream400-error boundary. It does not change attention, model weights, LRU insert/fetch/trim, active tensors, or sampling. Vision, tool calls and alternate templates are outside the qualified shared Rust surface. Real direct-backend probes verify rejected image/data-URL/content-part requests with zero fetches of a local canary URL, JSON/SSE answer/count/cache parity and bounded scalar snapshots.

Use `scripts/fixtures/mlx-0322-runtime-20261108.txt` for the separate34-package backend environment. Package pins are reproducibility inputs, not approval. `scripts/mlx-0322-contract.test.py` tests release/source and text boundaries without models; `BACKEND_PYTHON -I scripts/verify-kv-lru.py --wrapper scripts/mlx-retained-kv.py --release 0.32.2` checks real LRU byte enforcement using logical fixtures only. The default LRU command still checks the original0.32.0 wrapper. `scripts/token-counter-parity.py` compares complete IDs and fallback boundaries before admitting the Transformers5.22.2/tokenizers0.23.3/Jinja2 3.1.6 tuple in the Rust-owned CPU counter.

The unchanged original `qualify-model` runner is required: exact `--candidate-mlx-lm-version 0.32.2`, `--duration 1800 --warmup 30 --concurrency 1 --max-model-len 4096 --phase-samples 3 --request-timeout 60 --max-rss-growth-mib 256 --bind-architecture-evidence`, retained KV1536MiB and allocator512MiB. Do not use short-run exceptions or skip probes to issue startup evidence. The candidate flag does not promote the global version matrix; ordinary startup additionally verifies the complete matching evidence, identity, expiry and limits. A new path, source/environment, model, hardware or policy needs its own original qualification. Public JSON is not a portable startup approval.

日本語：元の資格確認済み環境を残し、別controllerと固定環境へ適用します。版番号と正確なソース組を同時に検査し、画像URL処理へ入る前にテキスト以外を拒否します。旧証跡の編集流用やガード回避は行いません。新しい完全な30分資格確認と共有APIの境界・追い出し・ライフサイクル検証が必要です。

简体中文：保留原有已认证环境，在独立controller及固定环境中应用。版本与精确源码组合同时校验，在图片URL处理前拒绝非纯文本消息。不修改旧证据来复用，不绕过门禁；必须重新完成完整30分钟认证及共享API边界、淘汰和生命周期验证。
