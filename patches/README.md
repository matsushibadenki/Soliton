# Local vLLM-Apple patches

`vLLM-apple/` is an independently managed, ignored checkout. Keep its changes reviewable and reproducible here rather than silently depending on an untracked local edit.

Patch base: `90f9c2bbd79c24d6d1f82122366e86345268b8b5`.

`vllm-apple-fingerprint.patch` changes only fingerprint path resolution/containment and regression tests. It retains complete-byte hashing, isolated probes, before/after file checks, inventory limits/recheck and rejection of external paths/symlinks. macOS uses the system strict `realpath` implementation; other platforms retain pathlib. Native canonical spelling can differ for case-aliased paths, so unchanged installation digests are compared explicitly for the measured environment. This is not a persistent cache or approval bypass.

Apply only to a compatible clean checkout with `git -C vLLM-apple apply --check ../patches/vllm-apple-fingerprint.patch`, then `git -C vLLM-apple apply ../patches/vllm-apple-fingerprint.patch`. If the check fails, reconcile the actual source rather than forcing application. Runtime source changes invalidate existing qualification evidence and require a fresh full qualification before inference. The frozen pre-change fingerprint worker in `scripts/fixtures/fingerprint-reference.py` is for differential measurements only; it is not used by Soliton at runtime.

日本語: vLLM-Appleは独立した管理対象のため、変更をパッチとして保持します。適用前に`apply --check`し、不一致を強制適用しません。全バイト検査・変更検出・上限・外部パス拒否は維持します。runtime変更後は新しい完全な検証証跡が必要です。

简体中文：vLLM-Apple为独立管理的检出目录，修改保留为补丁。应用前执行`apply --check`，不强制覆盖冲突。完整字节检查、变更检测、限制及外部路径拒绝保持启用。runtime修改后必须取得新的完整验证证据。
