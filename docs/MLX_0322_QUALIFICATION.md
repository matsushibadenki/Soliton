# MLX-LM0.32.2 guarded profile — 2026-11-08

## English

This is a separate opt-in Gemma2 2B 4bit plain-text profile: context4096, concurrency1, retained KV1536MiB, allocator free cache512MiB, MPS fallback1. The existing qualified0.32.0 controller/environment/evidence and ordinary settings are retained. Neither the global compatibility matrix nor the qualification runner is relaxed.

The canonical wrapper is [mlx-retained-kv.py](../scripts/mlx-retained-kv.py). It binds the exact release to its reviewed server/cache pair; it retains both previous0.32.0 pairs and adds only the measured0.32.2 pair. Unknown/development versions, changed bytes and incorrect release/source combinations fail. Full backend inventory and source/model/hardware/environment fingerprints still cover the entire selected installation. Package version labels and the two local hashes alone do not approve startup.

The LRU cache and Gemma2 implementation are byte-identical to the selected PyPI0.32.0 files. We reviewed the new model/tokenizer loading defaults, optional vision embeddings, cache-key path, response-state segmentation and validation boundaries. For plain text, embeddings remain absent and the existing cache/token path is preserved. The exact0.32.2 server's URL-inlining function is replaced with plain-string role/content validation inside its400-error boundary, so image URLs cannot silently enable a new input mode. Attention, weights, sampling and native LRU insertion/trim/fetch are unchanged. Real direct-backend testing rejects five unsupported message shapes with zero canary URL fetches, then verifies JSON/SSE answer/count/cache parity and KV/allocator caps.

The CPU counter additionally admits only the measured Transformers5.22.2/tokenizers0.23.3/Jinja2 3.1.6 tuple for its existing exact Gemma serialization. Previous tuples remain admitted. Complete token IDs match AutoTokenizer for211 raw inputs,844 chat variants and36 longer fixtures (1091 total); three invalid-role fixtures are rejected and six fallback guards pass. No heavy model framework is imported by this fast path.

The [fixed34-package snapshot](../scripts/fixtures/mlx-0322-runtime-20261108.txt) includes MLX-LM0.32.2/MLX0.32.4/Transformers5.22.2 in a new private Python3.12.16 environment. The [combined patch](../patches/vllm-apple-mlx-0322.patch) applies to clean controller commitbdef9567191bcc46154c0f7cf7f05089b718f0b4, independently of the existing controller. It was check-applied and compared exactly to the canonical wrapper. Do not force this patch onto an already modified tree or overwrite a qualified environment. See [patch instructions](../patches/README.md#mlx-lm0322-plain-text-profile--2026-11-08).

The unchanged original qualification command uses `--candidate-mlx-lm-version 0.32.2 --duration 1800 --warmup 30 --concurrency 1 --max-model-len 4096 --phase-samples 3 --request-timeout 60 --max-rss-growth-mib 256 --bind-architecture-evidence`, with the three profile environment variables above. Sampling/streaming promotion, multilingual quality, phase probes, RSS limit, process survival, clean shutdown and before/after identity checks remain mandatory. The short17-success preflight issued no startup evidence. The candidate-version flag only selects the exact installation to test; ordinary startup additionally verifies the full matching evidence, expiry and bounds.

## Verified result

🟢 [Done] The original full run passed1800.063 seconds:11871/11871 successful requests, zero failures, peak RSS growth327680 bytes against268435456 bytes allowed, process survival and clean shutdown. Before/after complete identity matched; text scope4096/concurrency1 evidence was issued. [Original report](evaluation/mlx-0322-qualification-20261108.json).

🟢 [Done] Shared Rust API36 cases plus EN/JA/ZH exact4096 admissions/4097 refusals all passed. Compared to0.1.4, all36 full answers, input/output/cached token counts, finish reasons and fixture matrix digest match. Five-history pressure verified cap1610612736 bytes and retained maximum1260593152 bytes; the old anchor returned8/3935 cached and immediate reheat3934/3935. Old0.32.0 evidence was refused and the new proof restored startup. [Gateway](evaluation/mlx-0322-gateway-20261108.json), [pressure/refusal/recovery](evaluation/mlx-0322-pressure-20261108.json).

🟢 [Done] Lifecycle: missing evidence is rejected at settings admission; expired evidence/over-qualified context cannot start; image/tool inputs reject before dispatch; active switch refuses; stream cancellation is recorded; owned model/controller/counter processes disappear; Strata mock switching and MLX restart work with a new session ID. Chat SSE, saved answer/metrics and preservation on stop pass. Ordinary state, old wrapper and old evidence byte hashes remain unchanged, and model root identity matches the old certificate. [Lifecycle](evaluation/mlx-0322-lifecycle-20261108.json), [summary, exact hashes and preservation](evaluation/mlx-0322-summary-20261108.json).

The new lifecycle harness initially assumed the wrong HTTP stage/status and error shape. Three failed attempts are [retained](evaluation/mlx-0322-lifecycle-attempts-20261108.json); its final expectations now match the existing early settings400 and busy-action400/error.message contract. Production guards, qualified wrapper and certificate were not changed for those corrections. These harness failures are not hidden as successful runs.

🟢 [Done] Five new source/text guard tests, two original source-pair tests,39 controller qualification/budget/evidence tests, real LRU logical fixtures,1091 ID comparisons/six fallback guards,168 Rust tests (one additional ignored test), all-target workspace check, frontend and server/Tauri builds passed. All QA owners are stopped. This is the0.1.5 source milestone; no new native GUI, signed installer, other-Mac or broad-model qualification is claimed.

## Reproduction and scope

On the measured Mac, the new controller is `.soliton-runtime/mlx0322-20261108/controller`, its backend Python/launcher are in `.soliton-runtime/mlx0322-20261108/python/bin`, and the local model remains `vLLM-apple/models/gemma-2-2b-it-4bit`. The controller uses the existing frontend Python `vLLM-apple/.venv/bin/python`; it does not install the controller's dependencies into the backend environment. The minimal private review artifact is `.soliton-runtime/mlx0322-20261108/profile.json`: it contains only the selected vLLM settings and qualification reference, without QA conversations or Strata mock settings. It does not apply ordinary settings; use the matching values in engine preferences.

After complete qualification, select the matching private evidence together with the controller, executable, model, context and budgets. A copied public report is not portable startup approval; changing any bound source/path/environment/model/hardware/policy needs a fresh qualification. Do not edit old evidence to reuse it. Native GUI use of this exact new profile, other Macs/models, vision/tools and broader output quality are separate checks; no general speedup is claimed.

The reusable probes all require explicit isolated settings and new output files:

- `BACKEND_PYTHON -I scripts/verify-kv-lru.py --wrapper scripts/mlx-retained-kv.py --release 0.32.2`: real LRU with logical-byte fixtures, no tensors/model.
- `FRONTEND_PYTHON scripts/verify-mlx-text-contract.py --isolated --controller /absolute/controller --executable /absolute/backend/bin/mlx_lm.server --model /absolute/model --output /absolute/new.json`: actual direct-backend text boundary/canary/cache check; never issues evidence.
- `node scripts/benchmark-engine-gateway.mjs --isolated --api http://127.0.0.1:18099 --output /absolute/new-gateway.json`:36 shared-API cases and EN/JA/ZH exact/over boundaries, requiring an empty dedicated offline host.
- `node scripts/verify-kv-pressure.mjs --isolated --api http://127.0.0.1:18099 --output /absolute/new-pressure.json --control-evidence /absolute/old-0320-evidence.json`: retained cap/eviction/reheat, old-installation evidence refusal and correct-profile recovery.
- `node scripts/verify-mlx-lifecycle.mjs --isolated --api http://127.0.0.1:18099 --output /absolute/new-lifecycle.json --strata-root /absolute/Strata --strata-python /absolute/Strata/.venv/bin/python --state-path /absolute/qa-state.json`: missing/expired/over-context evidence refusal, API text admission, cancellation, active switch refusal, owned child/counter cleanup and MLX restart. The Strata arm is a mock; it is not Strata model qualification. After nonmutation checks, this probe intentionally writes one synthetic QA chat turn and verifies its saved metrics.

Run model-owning probes serially, after the full qualification has stopped its worker. Keep normal settings and the old runtime/evidence intact. The public summary records exact hashes and the measured limits; a short preflight or passing unit test is not a replacement for the30-minute run.

## 日本語

既存0.32.0を残した別の任意プロファイルです。Gemma2 2B 4bit・テキスト・4096コンテキスト・同時実行1・保持KV1536MiB・allocator512MiB・MPS fallback1に限定します。版番号と正確なserver/cache組を同時に検査し、全インストール内容・モデル・環境・Macのidentity、期限、上限も維持します。0.32.2の画像URL処理へ入る前に非テキストメッセージを拒否し、新機能を自動的に有効にはしません。

計算コード・LRU本体は既存PyPI0.32.0と同一です。新しい生成／トークナイザー／応答状態処理を確認し、CPU計数の1091件の完全ID一致・6件のフォールバック検査、実モデルのJSON/SSE一致・URL取得ゼロ・予算上限を確認しました。30分再資格確認は元のランナーと完全identityで実施し、短時間結果から起動証跡は発行しません。新プロファイルの利用には対応するprivate controller／Python／証跡を組にし、通常設定や旧証跡は編集しません。速度向上や他モデル／他Macへの一般化は行いません。

🟢 [Done] 元の30分試験は1800.063秒・11871件成功・失敗ゼロ、ピークRSS増加327680 bytesで通過し、終了時のidentity一致・正常停止も確認しました。共通API36件と三言語の4096/4097境界、長履歴5本での追い出し／再加熱・旧証跡拒否／復旧、キャンセル・停止・切替・再起動・チャットSSE／保存が通過。0.1.4と全回答・入力／出力／cached数・終了理由が一致しています。通常設定・旧wrapper／証跡・モデルを保持し、Rust168件などの検証を終えて0.1.5の区切りとします。

⭕️ [Pending] 新プロファイルのネイティブGUI・他Mac／他モデル・署名／公証は別途確認が必要です。

## 简体中文

这是保留原0.32.0环境的独立可选配置，仅适用于Gemma2 2B 4bit纯文本、4096上下文、并发1、保留KV1536MiB、分配器缓存512MiB及MPS fallback1。版本与精确server/cache组合绑定，同时保持完整安装内容、模型、环境、硬件身份、有效期和上限校验。在0.32.2处理图片URL前拒绝非纯文本消息，不自动启用新输入模式。

Gemma2计算代码与LRU本体和已选PyPI0.32.0字节相同。审核了生成、分词器和响应状态变更；1091项完整token ID比较、6项回退门禁、真实模型JSON/SSE一致、零URL抓取与缓存上限检查通过。完整30分钟重新认证仍使用原运行器和完整identity，短测不签发启动证据。使用新配置需配套private controller、Python与证据，不修改普通设置或旧证据。没有通用加速、其他模型或其他Mac的保证。

🟢 [Done] 原30分钟测试实际运行1800.063秒，11871项全部成功、零失败，RSS峰值增长327680 bytes；结束时identity一致且正常停止。共享API36项、三语言4096/4097边界、五段长历史的淘汰／再加热、旧证据拒绝与恢复、取消／停止／切换／重启、聊天SSE及保存均通过；完整回答、输入／输出／缓存计数和结束原因与0.1.4一致。保留普通配置、旧wrapper／证据及模型，168项Rust测试等通过，形成0.1.5源码里程碑。

⭕️ [Pending] 新配置原生GUI、其他Mac／模型、签名和公证仍需分别验证。
