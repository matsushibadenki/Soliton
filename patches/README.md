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
