# Engine internal wall spans — 2026-11-10

## English

The primary4096-token Gemma2/MLX-LM0.32.2 path now has a separate instrumented candidate controller. The existing qualified runtime, backend wheel, model, evidence and ordinary preferences are preserved. This adds measurement, not a claimed inference speedup. SSD persistence and CLI subprocess spans remain separate work.

### Measured boundaries

| Span | Original operation | Interpretation |
|---|---|---|
| `qualification.verify` | Complete `verify_startup_evidence` | Includes the original evidence permission/age/digest/bounds/identity checks, nested validation and full identity computation |
| `qualification.validate` | `validate_evidence` | Includes repeated calls; nested in verification, not additive to its total |
| `qualification.model_identity` | `build_model_integrity_manifest` | Full model-file integrity work, not cached approval |
| `qualification.backend_identity` | `fingerprint_backend` | Full selected installation fingerprint and isolated probe |
| `cache.fetch` | `LRUPromptCache.fetch_nearest_cache` | RAM LRU lookup/extraction; not SSD load |
| `cache.insert` | `LRUPromptCache.insert_cache` | RAM cache registration; may contain trim; not disk save |
| `cache.trim` | `LRUPromptCache.trim_to` | Includes initialization/no-op calls and may nest in insert |
| `batch.prefill` | `PromptProcessingBatch.prompt` | Includes existing eval/cache-maintenance work and empty calls; the final prompt-token step can occur in decode |
| `batch.decode` | `GenerationBatch.next` | Includes host work, original synchronization and response construction; not GPU-kernel time |
| `sequential.prefill_first` | First `stream_generate` iterator advance | Prefill **plus first token** and detokenization, not pure prefill |
| `sequential.decode` | Subsequent iterator advances | Excludes time suspended in the consumer, includes normal final exhaustion; calls are not token counts |

All values are host monotonic wall time. No extra MLX evaluation, GPU synchronization, model/cache mutation, flush or sampling change is introduced. Overlapping/nested spans cannot be summed to produce total request time. No throughput is inferred from these method-call counts. A very short or zero duration can be a real no-op, while an unexecuted span has `calls:0`, `last_ms:null`, `max_ms:null`. Failed operations propagate their original exception and record a failure without retaining its message.

The fixed11-row ledger stores calls/failures/total/last/max only. It retains no request text, token IDs, tensors, model outputs or unbounded sample history. A mutex makes each snapshot internally consistent; it does **not** make the snapshot atomic with request completion. Cache insertion can finish after an HTTP response. Cumulative values belong to one process, not an individual request; only deliberately isolated intervals can be compared. The generation-thread cache lock is unchanged. Cache method timers run inside the existing retained-budget lock, so they exclude waiting to acquire that wrapper lock; nested calls retain their original ordering. The added host clock/mutex overhead has not been established as zero or characterized under broad concurrency.

### API and identity

- Controller `/v1/runtime.startup_internal_spans`: qualification ledger.
- Private backend `/vllm-apple/kv-cache.internal_spans`: cache/inference ledger, alongside the existing bounded KV/allocator counters.
- Existing authenticated Rust `refreshBackendBudget` action: normalizes both into `/studio/state.modelPlan.backendBudget.startupSpans` and `backendSpans`, with camelCase scalar names. It retains the existing2-second/64KiB aggregate request limit, exact saved-settings/live-process/session checks and30-second diagnostic expiry. Stop/settings mismatch/expired samples hide the report. Old controllers return null new fields.
- Schema `soliton-internal-wall-spans-v1`, basis `host_wall_no_added_sync`, consistency `atomic_ledger_not_request_atomic`. Rust admits only the fixed names and finite/nonnegative/consistent scalar values; missing or malformed ledgers remain unknown, without inventing zero-cost work.

The new launcher admits only MLX-LM0.32.2 with the exact existing server/cache pair plus `generate.py` SHA-256 `9d46803799f99d6fc7c35fff2d4357c79b1a25ae98cc63efeaaebeb50a7b5f33`. Every source guard precedes hook installation/model startup. The entire controller source, model, installation, policy, environment and hardware identity remains bound by the original qualification mechanism; old evidence cannot approve this new runtime. No version-matrix promotion or short-run exception is added.

### Reproduction

Apply [the incremental patch](../patches/vllm-apple-internal-spans.patch) after the separate0.32.2 patch to a clean **separate** controller; [patch instructions](../patches/README.md#internal-wall-spans--mlx-lm0322--2026-11-10). The new canonical launcher is [internal spans](../scripts/mlx-internal-spans.py). The candidate budget helper is provided by the incremental patch; the original canonical [budget wrapper](../scripts/mlx-retained-kv.py) remains unchanged. Installed wheels are not edited. Compare applied bytes before testing.

CPU tests: `python3 scripts/mlx-internal-spans.test.py`, `python3 scripts/mlx-0322-contract.test.py`, and supported-Python Rust tests. Controller evidence/backend/allocator/KV tests must pass with the candidate launcher expectation; synthetic tests issue no qualification evidence.

Use the unchanged original `qualify-model` runner with exact candidate0.32.2, context4096/concurrency1, KV1536MiB/allocator512MiB/MPS fallback1, duration1800/warmup30, phase samples3, request timeout60, RSS growth limit256MiB and `--bind-architecture-evidence`. Do not overlap model-owning probes, alter old evidence or reuse a shortened result for startup.

After qualification stops its worker:

1. `FRONTEND_PYTHON scripts/verify-internal-spans.py --isolated --before /absolute/old-controller --after /absolute/candidate-controller --executable /absolute/backend/bin/mlx_lm.server --model /absolute/model --fixtures /absolute/six-fixtures.json --output /absolute/new-parity.json`: serial24-request-per-controller EN/JA/ZH, JSON/SSE, ordinary batching/seeded sequential parity. The fixed six fixtures can be exported from `cases()` in `benchmark-engine-gateway.mjs` at indices0,5,6,11,12,17. Compare complete answers/usage/finish reasons; inspect all seven inference/cache spans. This direct-backend probe does not issue evidence.
2. Start a dedicated Rust host with empty isolated state and matching candidate settings/evidence. `node scripts/verify-internal-spans-managed.mjs --isolated --api http://127.0.0.1:18099 --output /absolute/new-managed.json` verifies actual full startup verification spans, public normalization, six multilingual JSON/SSE responses, unchanged conversations, session binding and stop clearing.
3. Run the existing gateway36-case/exact4096-over4097 suite and retained-cache pressure/old-evidence refusal/recovery checks before adoption. Real-model evidence and results are recorded separately; passing source checks alone is not compatibility certification.

## 日本語

4096トークンのGemma2／MLX-LM0.32.2主経路へ、独立した計測候補を追加しました。既存の資格確認済み環境・重み・証跡・通常設定を残します。資格確認全体とモデル／backendの完全な指紋検査、RAMキャッシュの検索・登録・整理、バッチprefill/decode、逐次生成の初回／後続イテレーター処理を計測します。SSD保存／復元・CLIは別工程です。

値はCPU側の壁時計・プロセス累積値です。GPUカーネル時間や要求単位の値ではなく、入れ子の区間を足し合わせたり、呼出し数からtoken/sを算出したりしません。逐次初回には最初のトークンまでを含み、バッチprefillには空処理も含みます。追加のGPU同期・eval・flushや生成条件の変更は行いません。計測用時計・ロックのコストがゼロとは主張しません。

固定11行のスカラーだけを保持し、プロンプト・token ID・テンソル・例外メッセージを保存しません。未実行はnullを維持します。Rustは既存の認証付き診断操作から取得し、形式・数値整合性・現在の設定と所有プロセスを検査します。停止・設定変更・期限切れでは表示しません。既存controllerは新しい項目がnullのまま使用できます。UI表示は今回追加していません。

server/cache/generateの正確なソースと版を検査し、元の完全identity・期限・上限検査と30分資格確認を維持します。短時間試験から起動証跡を作らず、新しい候補の証跡と変更前後の多言語・JSON/SSE・キャッシュ・停止／再起動確認を揃えて判断します。速度改善を示す変更ではありません。

## 简体中文

为4096上下文的Gemma2／MLX-LM0.32.2主路径新增独立计时候选，保留原认证环境、模型、证据和普通设置。记录完整启动资格验证、模型及backend完整指纹、RAM缓存查找／插入／整理、批处理prefill/decode以及逐次生成首次／后续迭代耗时。SSD保存／恢复与CLI属于后续独立工作。

这些值是进程累计主机壁钟时间，不是GPU内核或单请求计时。嵌套区间不能相加，方法调用次数也不能作为token/s的分母。逐次首次计时包含首token；批处理prefill包含空调用。不增加GPU同步、eval、flush或修改采样条件，不声称计时锁与时钟调用没有成本。

固定11行仅保存计数及累计／最近／最大耗时，不存储提示词、token ID、张量或异常内容。未执行项保持null。Rust沿用已认证、限时限量且绑定设置／存活进程的诊断路径，检查schema和数值一致性；停止、设置不匹配及过期后不显示。旧controller的新字段仍为null，不新增UI。

严格绑定server/cache/generate源码及版本，保留完整identity、有效期、上下文上限和原30分钟资格验证；短测不签发启动证据。新候选须配套证据并通过多语言JSON/SSE、缓存和生命周期测试。本次新增可观测性，不宣称推理提速。

## Verified observations — 2026-11-10

- 🟢 [Done] Original full qualification:1800.111s,14,588/14,588 successes, zero failures,131,072-byte peak RSS growth against268,435,456 bytes allowed, clean shutdown and fresh identity-bound evidence. Installed backend/model/environment/hardware identities match the prior0.32.2 run; only controller runtime identity changed. [Public summary](evaluation/internal-spans-qualification-20261110.json).
- 🟢 [Done] Fresh serial old/candidate comparison:24 requests each, EN/JA/ZH × short/long × unseeded batch/seeded sequential × JSON/SSE; complete answers, usage and finish reasons match. All seven inference/cache spans execute without failures; both owned workers stop. [Parity](evaluation/internal-spans-parity-20261110.json).
- 🟢 [Done] Shared Rust API36 cases and three exact4096/over4097 boundaries passed. All36 full answers/usage/finish reasons and boundary records match the prior0.32.2 run. [Gateway](evaluation/internal-spans-gateway-20261110.json).
- 🟢 [Done] Actual managed startup verification and normalized internal ledgers, six multilingual JSON/SSE requests, unchanged conversations and stopped-diagnostic clearing passed. One observed startup: verification2544.66ms, model manifest1531.45ms, backend fingerprint972.46ms. Six short requests: cache fetch1.25ms /insert0.41ms /trim0.02ms cumulative, batch prompt196.70ms /decode617.27ms cumulative. Parent/child intervals overlap; these are descriptive host-wall observations, **not** controlled latency improvements or GPU time. [Managed observations](evaluation/internal-spans-managed-20261110.json).
- 🟢 [Done] Nine-request five-history pressure/eviction/reheat and recent-history reuse: retained bytes stay below1536MiB, revisit cached8 → reheat3934; old-controller evidence is refused and correct-profile startup recovers. [Pressure](evaluation/internal-spans-pressure-20261110.json).
- 🟢 [Done] Managed lifecycle: twelve checks for JSON/text admission, active-switch refusal, cancellation, owned tree/counter cleanup, Strata mock switch, session change/restart, API history, chat SSE/metrics/disk persistence and stop preservation; three missing/expired/over-context evidence refusals pass. [Lifecycle](evaluation/internal-spans-lifecycle-20261110.json).
- ⭕️ [Pending] Other Macs/models, broad concurrency/overhead comparison, native GUI for this exact candidate and signed/notarized distribution. SSD persistence/CLI timing remains unfinished.

Two early gateway attempts stopped before model startup because the hand-written QA state lacked required fields; the settings action also correctly refused a direct engine change. The dedicated state was corrected through the authenticated full-settings action and explicit switchEngine operation. Failed artifacts remain private; no load/selection guard was relaxed. Original canonical wrapper,0.32.2 patch, profile and evidence are unchanged. The original installation remains selected in ordinary settings; the new profile is opt-in.

日本語：元の30分試験は1800.111秒・14,588件すべて成功・失敗ゼロ、RSS最大増加128KiB、正常停止で通過しました。変更前後各24件の全回答・usage・終了理由、共有API36件と三言語4096/4097境界が一致しました。Rust管理下で資格確認内部時間、キャッシュ・prefill/decodeのスカラー取得と停止時消去を確認。長履歴の追い出し・再加熱、旧証跡拒否と復旧も通過しています。実測値はこのMacでの記述的な観測で、速度改善やGPU時間とは扱いません。初期QA設定の不足は認証付きAPIと正規切替で修正し、通常設定・既存環境は保持しています。

简体中文：原30分钟测试通过，实际1800.111秒、14,588项全部成功、零失败、RSS峰值增长128KiB且正常退出。前后各24项完整回答／usage／结束原因一致；共享API36项与三语言4096/4097边界一致。已通过Rust管理下资格验证内部时间、缓存及prefill/decode标量获取、停止后清除诊断，以及长历史淘汰／再加热、旧证据拒绝与正确配置恢复。上述仅为本机描述性壁钟观测，不代表提速或GPU时间。初始QA配置缺项通过认证API及正常切换修正，保留普通设置与原环境。

The measured opt-in private profile is `.soliton-runtime/internal-spans-20261110/profile.json`; it points to the new controller and fresh matching evidence without applying ordinary preferences. CPU/regression checks:166 Rust,4 new span tests,5 existing0322 contracts,75 controller evidence/backend/KV/API tests and5 gateway-tool tests; frontend/server builds, Tauri check and exact seven-file patch replay passed. The QA host and all owned model/counter processes stopped.

日本語：キャンセル・排他切替・停止／再起動・会話保存など12項目と証跡拒否3条件も通過。QAホストと所有モデル／カウンターは停止済みです。任意プロファイルはprivateの`profile.json`に記録し、通常設定へは適用していません。

简体中文：取消、排他切换、停止／重启和会话保存等12项及3种证据拒绝条件均通过；QA主机和所有模型／计数进程已停止。可选配置保存在private的`profile.json`，未应用到普通设置。
