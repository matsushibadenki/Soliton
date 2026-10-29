# Local inference measurements

Use an existing prepared model and the running Studio app or standalone gateway. The script never prepares/downloads a model, changes settings, or writes chat messages. It sends only generated synthetic text through the authenticated loopback API. Mock engines are rejected.

```sh
npm run benchmark:inference -- --start --output /tmp/strata-benchmark.json
npm run benchmark:inference -- --suite workload --languages en,ja,zh --start --output /tmp/soliton-workload.json
npm run test:benchmark
```

Without `--start`, the engine must already be ready. With it, an offline engine is started and stopped afterward, including on HTTP errors, timeout, SIGINT or SIGTERM. An engine that was already running stays running. SIGKILL or machine power loss cannot run cleanup. Stop errors are reported explicitly; inspect the Engine tab if cleanup fails. Do not run alongside another inference client.

Options:

| Option | Default | Meaning |
| --- | --- | --- |
| `--url` | `http://127.0.0.1:8080` | Plain IPv4 loopback gateway URL only |
| `--suite` | `retrieval` | `retrieval` or multilingual `workload` |
| `--languages` | `en` / workload: `en,ja,zh` | Unique subset of English, Japanese, Simplified Chinese |
| `--rows` | retrieval: `0,64,256`; workload: `0,16,32` | 1–8 lengths, each with 0–512 synthetic filler rows |
| `--timeout` | `240000` | Maximum milliseconds per request and startup readiness wait; 1000–900000 |
| `--start` | off | Start only if offline, and restore stopped state afterward |
| `--output` | none | New JSON file; existing files are never overwritten |

Each request asks for a known code inserted halfway through the filler records. Temperature is 0, reasoning is explicitly disabled, output is capped at 64 tokens. `rows` and `inputBytes` describe input construction, not token counts. `usage` and `timings` are copied from the engine response; absent fields remain absent/null. Elapsed time includes gateway/HTTP overhead. Retrieval reports `exact`, `contains`, `incorrect` or `unanswered`; completion status alone does not mean retrieval was correct. This small synthetic task is not a general model accuracy evaluation.

The workload suite runs each requested retrieval length in each language, then one three-message recall request per language. The assistant acknowledgment is synthetic; this tests supplied conversation context, not a preceding live generation. Filler records remain English while instructions and recall turns are localized. The default workload contains 12 cases. Context/budget rejections are reported separately from incorrect answers and do not stop the remaining workload cases. Transport errors, timeout and memory guards stop the run. `passed` requires every planned case to complete with the exact code and no cleanup error; the CLI exits 1 otherwise. Partial coverage is never presented as a passing evaluation. Raw upstream error bodies and generated responses are excluded; only bounded status and allowlisted rejection codes are retained.

Qualified MLX requests pass the gateway’s local tokenizer/template preflight; HTTP elapsed time includes initializing the CPU counter on the first request, then exact counting through the same engine-session counter. Its memory belongs to the app’s children, outside the model-daemon tree reported as engine memory; system samples include it. No second model is loaded. Explicit output budgets are required, and context rejection occurs before inference/history writes.

For vLLM-Apple, the report records its selected context and engine. Strata-specific memory policy is omitted; a saved Strata mock preference does not mark the selected real vLLM backend as mock. Evidence startup refusals have stable reason codes without exporting process logs.

Mac and engine memory are sampled sequentially about once per second, using the gateway's cached native samples. The report includes timestamps and sampled peaks/minimum availability; brief peaks may be missed. Mac memory includes other apps and reclaimable caches. Engine memory sums physical footprints charged to the owned server's process tree, including its native Metal child and lifecycle watcher; shared allocations can be charged to multiple processes. It is not unique physical pages or model file size. The report includes `processCount` and `rootBytes` to distinguish the tree from Python alone. Do not compare legacy parent-only measurements with the new descendant totals.

The script stops measurement if reported available system memory falls below 2 GiB. This is a sampling guard, not a guarantee against memory pressure or swap. Requests stop at their timeout; partial failures remain in the JSON. Report files contain no chat text, model output, session key or full settings. They do include machine capacity, model ID and memory policy. Cold/warm caches and other processes can affect results; cases run sequentially without flushing caches.

日本語: `--suite workload` は英語・日本語・简体中文の検索と3メッセージの記憶確認を行います。コンテキスト制限による拒否は誤答と分けて記録し、全ケースの完全一致だけを合格とします。既存モデルを再利用し、短い入力から段階的に測定します。トークン数はエンジンの報告値のみで、行数から推定しません。メモリは約1秒間隔の標本値で、瞬間的なピークや共有領域の重複計上に注意してください。会話履歴と設定は変更しません。停止中から起動した場合は終了時に停止へ戻します。

简体中文：`--suite workload` 检查英日中文检索及三条消息的上下文记忆。上下文限制拒绝单独记录，只有所有计划用例完全匹配才通过。重复使用已有模型，从较短输入逐步测量。token数仅采用引擎报告值，不按行数估算。内存约每秒采样，可能遗漏瞬时峰值，共享分配也可能重复计入。不会修改对话和设置；从停止状态启动的引擎会在结束后停止。

## Apple M4 / 32 GiB observations · 2026-10-17

Existing `qwen3.8-flash-next-q2_0`, automatic 8,192 context, prompt chunk 64, 7 GB reserve, SSD streaming. No model downloads. Initial sequential run:

| Filler rows | Reported input / output tokens | HTTP elapsed | Retrieval |
| --- | --- | --- | --- |
| 0 | 35 / 8 | 15.651 s | Exact |
| 64 | 1,050 / 8 | 76.827 s | Exact |
| 256 | No final usage; engine log said 4,279 input tokens | 240.185 s, timeout | Not evaluated |

The initial run used parent-only engine memory; those memory samples are excluded from the [initial timing evidence](benchmarks/metal-m4-32g-20261017-initial.json). Timeout does not establish a retrieval/accuracy failure. The script restored the stopped engine state. This is one sequential run under other desktop activity, not an isolated performance comparison or a claim that the context limit was reached.

The [corrected-group-memory rerun](benchmarks/metal-m4-32g-20261017.json) used the same model and policy, with cases 0 and 64 and a 120-second request deadline. Both returned the exact code: 35/8 tokens in 10.913 s and 1,050/8 tokens in 89.637 s. All 99 engine memory samples counted two processes. Sampled group footprint peaked at 7.62 GiB; minimum reported Mac availability was 4.48 GiB. These are summed per-process footprint samples, not unique physical memory or continuous peaks. Different timings between runs are observations under desktop activity, not evidence that memory accounting changed inference performance.

Native Engine preferences showed the group total and graph, then “—” after the engine stopped. The full document (including six original messages, conversation summaries and layout) and settings matched the pre-run snapshot. No model downloads occurred. Verification: 55 Rust tests, nine benchmark HTTP/aggregation tests and Apple Silicon release build passed. Larger/sustained context, broader accuracy, SSD-throughput isolation and other Mac capacities remain unverified.


## Apple M4 / 32 GiB · qualified MLX text · 2026-10-22

Gemma 2 2B 4-bit, MLX 0.32.2 / MLX-LM 0.32.0, context 1024, concurrency 1, temperature 0, output budget 64, no model downloads. The native Tauri runtime used an isolated copy of the existing state; six messages, all settings, and the ordinary state file were preserved. Each run restored the stopped engine state.

- [Default workload](benchmarks/vllm-m4-32g-workload-20261022.json): 12/12 exact, zero refusals or errors. EN/JA/ZH retrieval at 0/16/32 rows plus synthetic three-message recall per language. Engine-reported input was 27–537 tokens. HTTP elapsed was 4.704–7.500 seconds, including local tokenizer startup; this is not raw engine latency.
- [Boundary run](benchmarks/vllm-m4-32g-boundary-20261022.json): 12 exact completions and three 64-row requests rejected with HTTP 422 `context_length_exceeded`. No incorrect answers, transport failures or timeouts. The report’s strict `passed` is false because three planned inference cases were rejected; rejection is neither model inaccuracy nor completed coverage.

- [Near-limit run](benchmarks/vllm-m4-32g-near-limit-20261022.json): three 58-row retrievals and three multi-turn requests completed exactly; the three 64-row requests were rejected before inference. Accepted retrievals reported 946–953 input tokens, plus a reserved output budget of 64, within 1024. Six exact completions, three refusals, no errors/timeouts; strict `passed=false` again preserves the missing-coverage distinction.

The initial exploratory 64-row run exposed 1042–1049 engine-reported input tokens despite a selected context of 1024. The backend lacks `/tokenize`, and the daemon's unknown-count fallback did not prevent these requests. Those initial reports are excluded from the validated results above. Soliton now counts the local chat template and requested output before GUI/API inference and rejects unavailable counting instead of guessing. Local counts matched all 15 exploratory engine prompt counts. Native API and GUI overflow rejection also left history unchanged. Startup evidence checks and the upstream checkout were preserved.

These initial measurements used a new tokenizer process per request and include several seconds of initialization each time. The subsequent reuse comparison below retains the same exact-count gate. These synthetic tasks are not general quality, throughput or broad long-context certification.

日本語: 通常12ケースはすべて完全一致。64行の3ケースは上限超過として推論前に拒否され、誤答には数えません。strict `passed=false`は未実行ケースを合格扱いしないためです。事前計数の起動時間を含む測定で、純粋なモデル速度とは比較できません。

简体中文：默认12个用例全部完全匹配。三个64行用例在推理前因上下文超限被拒绝，不计为错误答案；strict `passed=false`表示计划用例未全部执行。耗时包含本地计数进程的启动，不能作为纯模型速度比较。


## Token-counter reuse comparison · 2026-10-22

The same M4 / 32 GiB, Gemma 2 2B 4-bit, MLX 0.32.2 / MLX-LM 0.32.0, identity-bound evidence, context 1024 and concurrency 1 were used in both runs. No model/backend dependencies or upstream startup checks changed. Each run sent the same EN/JA/ZH retrievals at 0/32/58/64 rows and three-message recall, twice: 30 requests, 24 exact completions, six expected context refusals. Input/output token counts for all 24 completions and all 30 accept/refuse decisions matched. Reports store no prompt or generated text.

| HTTP measurement | Before: one process/request | After: one counter/engine session |
| --- | ---: | ---: |
| First accepted request | 6.912 s | 5.583 s |
| Median accepted request, excluding first | 8.383 s | 1.366 s |
| Mean accepted request, excluding first | 8.992 s | 1.634 s |

Warm median decreased 83.70% (6.13×). These are end-to-end HTTP measurements, including inference and transport, under ordinary desktop activity. They do not isolate GPU execution or certify performance across machines. The first request still imports Transformers and loads the exact tokenizer/template; subsequent requests reuse it without caching prompts, answers or counts. The retained counter's sampled RSS was 460.47 MiB; RSS is not unique additional physical memory or a continuous peak. It retains no model weights and is reaped at engine stop/switch/settings changes.

The after-run also killed the actual counter: two subsequent requests rejected with 503 rather than falling back; explicit engine restart recovered through unchanged evidence checks. Whole process trees exited after stop, no counter/model was present while Strata mock ran, and SIGTERM shut down both the counter and model without orphaning workers. Conversation/settings and ordinary state bytes were preserved. Strata switching here is protocol/lifecycle coverage using the mock, not Strata real-model quality certification.

Native regression also passed evidence absence/expiry/over-limit refusal, verified startup, EN/JA/ZH GUI/API generation, cancellation, worker cleanup, switching and persistence. Verification: 68 Rust tests, 16 benchmark HTTP tests, 32 upstream compatibility/evidence/fingerprint tests and the Python counter protocol/file-change/EOF test.

A preliminary run was discarded after finding an orphan from earlier testing. Further harness setup failures were kept outside the published measurements. Both published runs started with no existing inference processes; the final harness validates the isolated Strata mock paths before measurement.

[Before report](benchmarks/token-reuse-before-20261022.json), [after report](benchmarks/token-reuse-after-20261022.json), [validated comparison](evaluation/token-reuse-comparison-20261022.json).

Reproduce with a frozen pre-change native binary and the current native bundle. Port 8080 must be free and all inference processes stopped; the script refuses otherwise. Reports must be new files. It uses an isolated state copy, existing model/evidence, and an explicitly configured Strata mock for lifecycle checks:

```sh
npm run benchmark:token-count -- before /absolute/path/to/frozen-baseline-binary /tmp/token-before.json
npm run benchmark:token-count -- after "$PWD/target/aarch64-apple-darwin/release/bundle/macos/Soliton.app/Contents/MacOS/soliton" /tmp/token-after.json
npm run benchmark:token-count:compare -- /tmp/token-before.json /tmp/token-after.json /tmp/token-comparison.json
npm run test:token-counter
cargo test -p studio-core --lib
```

`SOLITON_NATIVE_STATE` selects a different source state; `SOLITON_REAL_EVIDENCE` selects an existing complete qualification report. No settings or dependencies are installed or changed in the source state. The comparison asserts identical evidence/settings fingerprints and token counts, not just successful HTTP responses. A comparison pass expects the six 422 refusals; the general workload benchmark's strict all-cases-complete pass policy is unchanged.

日本語: 再利用後の実モデル応答中央値は8.383秒から1.366秒へ83.7%短縮しました。初回読み込みは残り、計数専用CPUプロセスのRSS標本は460.47 MiBです。重みは保持せず、停止・切り替え時に終了します。成功24件のトークン数と全30件の許否判定は一致し、異常時は再起動まで拒否します。互換性検査は変更していません。

简体中文：复用后的实际模型响应中位数从8.383秒降至1.366秒，缩短83.7%。首次加载仍保留；计数用CPU进程的RSS采样为460.47 MiB，不保留模型权重，停止或切换时退出。24次成功请求的token数及全部30次允许/拒绝决策一致，异常后保持拒绝直到重启，兼容性检查保持不变。

## Token counter first response and memory — 2026-10-22

The counter now has a lightweight path for the **exact measured Gemma 2 serialization and dependency versions**. SHA-256 checks cover `tokenizer.json`, `tokenizer.model`, `tokenizer_config.json` and `special_tokens_map.json`; unknown sidecars or different versions select the existing offline AutoTokenizer. It uses the same serialized Hugging Face tokenizer and original sandboxed Jinja template, without importing Transformers or inference frameworks. Startup qualification, expiry/identity checks, per-count file stamps, failure/cancellation handling and stop-before-switch remain enabled. This is a tokenizer implementation allowlist, not permission to run an unqualified backend.

Same existing Gemma 2 2B 4-bit model, M4 / 32 GiB, context 1024, concurrency one, identical evidence/settings. No dependency/model/backend installation changed. The native before/after measurement starts with a fresh counter after engine readiness; OS file caches remain warm. Both runs completed 24 exact EN/JA/ZH retrieval/multi-turn responses and six matching context refusals, with identical reported prompt/completion/total token counts for all accepted pairs.

A separate **first-content (TTFT) comparison** uses the same shared Rust core in matched headless debug builds on port 18084, isolating it from the ordinary GUI on 8080. Each version starts one fresh counter after verified engine readiness. First nonempty streamed content: **5.570→0.971 s (82.6% lower)**; counter RSS after the multilingual cases: **619.13→185.64 MiB (70.0% lower)**. Both versions produced the exact `cobalt` response, all 24 subsequent exact answers/six refusals with matching usage, and accepted the exact context boundary while rejecting one extra token. Stop/switch/failure/restart/signal cleanup and ordinary-file/evidence preservation passed. The headless warm complete-response median was 1.304→1.330 s. See [stream comparison](evaluation/token-footprint-stream-comparison-20261022.json), [before](benchmarks/token-footprint-headless-stream-before-20261022.json) and [after](benchmarks/token-footprint-headless-stream-after-20261022.json). TTFT is one observation per version, excludes startup qualification, retains OS caches and does not certify all workloads or other Macs. Raw engine-start-to-first-content times are retained in the reports, but their differing startup durations are not attributed to this tokenizer change.

| Measurement | Before: reused AutoTokenizer | After: qualified lightweight path |
| --- | ---: | ---: |
| Native first request, complete HTTP response | 3.352 s | 1.946 s (42.0% lower) |
| Native counter RSS after 30 cases | 616.00 MiB | 60.27 MiB (90.2% lower) |
| CPU worker first count, median of five fresh processes | 12.975 s | 2.183 s (83.2% lower) |
| CPU worker RSS immediately after first count, median of five | 597.94 MiB | 186.45 MiB (68.8% lower) |
| CPU warm count, median of five 20-request medians | 16.662 ms | 16.317 ms |
| Native warm complete-response median | 1.052 s | 1.645 s |

The native warm response was **slower** in this pair; these measurements do not establish faster warm model inference. Desktop load differed substantially (one-minute load averages at run start/end: before 7.77/6.11, after 12.11/23.64). CPU-only trials alternate before/after implementations and include no model engine. Their reference is a benchmark-only copy of the same worker using the original AutoTokenizer branch; the native before measurement uses the frozen pre-change release binary. The two measurements have different conditions and must not be combined into an end-to-end total. Counter RSS is a sample, not peak/physical footprint/unique added pages; OS reclamation, sharing and memory pressure can affect these samples; immediate-first-count and later retained samples require separate reporting. No second resident model or request/answer/count cache is introduced.

Exactness verification compares **full token-ID arrays**, not only lengths: 211 raw inputs and 844 chat variants (EN/JA/ZH, combining characters, emoji, whitespace, special tokens, long/random text, multi-turn and both thinking flags) match AutoTokenizer. Three malformed-role conversations reject on both paths. Unknown sidecar, four changed serializations and changed dependency version force fallback; a live lightweight worker rejects after its files change. Raw completion preserves the actual Transformers 5 Gemma BOS postprocessor, even though its config flag alone would suggest otherwise. The release binary was checked to embed the exact tested worker source.

Native lifecycle checks passed: one counter, failure latches 503 without auto-respawn, explicit restart recovers, stop/switch reap the entire tree, Strata mock starts without an MLX/counter process, and SIGTERM reaps the tree. Ordinary history/settings bytes and qualification evidence remain unchanged. Strata here is lifecycle coverage using its mock, not a real Strata model measurement. Verification also passed 68 Rust tests, 16 HTTP benchmark tests, 32 upstream compatibility/evidence/fingerprint tests, the upstream Strata contract test, and the Python protocol/file-change/EOF test.

Reports: [native comparison](evaluation/token-footprint-comparison-20261022.json), [before](benchmarks/token-footprint-before-20261022.json), [after](benchmarks/token-footprint-after-20261022.json), [alternating CPU trials](benchmarks/token-footprint-cpu-20261022.json), [full token-ID parity](evaluation/token-footprint-parity-20261022.json). Additional stream-baseline attempts returned [HTTP 503](benchmarks/token-footprint-stream-before-20261022.json) and [connection refusal](benchmarks/token-footprint-stream-before-retry-20261022.json); neither is a passing run or a valid first-token timing. A further [native stream trial](benchmarks/token-footprint-stream-after-20261022.json) completed the model/boundary checks but detected external GUI conversation changes and is excluded as a passing run. Failed/interfered reports are retained. The isolated headless pair above supplies the passing TTFT evidence. The first-request value in the table is completion latency, not TTFT, and excludes engine startup/qualification. The benchmark now retains private isolated state for recovery if external document changes are detected, instead of deleting it or printing conversation contents.

Reproduce sequentially, with no other inference engine running. Keep a pre-change release binary before rebuilding; use new output filenames. For headless TTFT, freeze matched before/after `cargo build -p studio-server` binaries, set `SOLITON_BENCHMARK_RUNTIME=headless SOLITON_FIRST_STREAM=1` for both benchmark runs, and use comparison mode `stream`. Headless signal coverage uses SIGINT; native coverage uses SIGTERM. CPU/parity checks must use the qualified backend Python, not an unrelated Python installation:

```sh
node scripts/token-count-benchmark.mjs before /tmp/soliton-before-footprint /tmp/native-before.json
npm run desktop:build
node scripts/token-count-benchmark.mjs after target/aarch64-apple-darwin/release/bundle/macos/Soliton.app/Contents/MacOS/soliton /tmp/native-after.json
node scripts/compare-token-count.mjs /tmp/native-before.json /tmp/native-after.json /tmp/native-comparison.json footprint
/opt/homebrew/opt/vllm-metal/libexec/bin/python scripts/token-counter-parity.py vLLM-apple/models/gemma-2-2b-it-4bit
/opt/homebrew/opt/vllm-metal/libexec/bin/python scripts/token-counter-startup.py --auto-tokenizer crates/studio-core/src/token_budget.py vLLM-apple/models/gemma-2-2b-it-4bit /tmp/cpu-startup.json
```

日本語: 画面と干渉しない共有Rustサーバーでは、初回の文字到着が5.57→0.97秒（82.6%短縮）、計数RSSが619→186 MiB（70.0%減）でした。上限ちょうどの受理と1トークン超過の拒否、排他稼働とプロセス回収も通過しました。同じ実モデルでネイティブ版の初回HTTP応答完了は3.35→1.95秒、30件後の計数プロセスRSSは616→60.3 MiBでした。計数単体を交互に5回測った中央値は初回12.97→2.18秒、初回直後のRSSは598→186 MiBです。測定時点・負荷が異なるため、二つのRSS値を同じ基準で比較しないでください。1055件の完全なトークンID列が一致し、互換性・証跡・上限超過拒否・異常時の拒否・エンジン排他稼働を維持しています。ウォーム時の実モデル応答は今回遅く、ウォーム推論の高速化は確認していません。初回の値は応答完了までで、最初のトークン到着やエンジン起動時間とは異なります。

简体中文：同一实际模型的首次HTTP响应完成时间为3.35→1.95秒，30次请求后计数进程RSS为616→60.3 MiB。交替进行五次独立计数进程测量，首次计数中位数为12.97→2.18秒，首次计数后RSS为598→186 MiB。两个RSS指标的采样阶段及系统负载不同，不能混为同一基准。1055组完整token ID完全一致，兼容性、证据、超限拒绝、异常拒绝及引擎互斥保持启用。本次热态模型响应较慢，未证实热态推理加速。表中首次响应值为完整响应耗时，不是首token延迟，也不含引擎启动时间。独立端口的共享Rust服务器测得首个流式内容5.57→0.97秒（降低82.6%），计数RSS619→186 MiB（降低70.0%）；验证上下文边界刚好可接受，超出一个token则拒绝，并通过引擎互斥及进程回收检查。

## Qualified startup overlap — 2026-10-23

Prioritize useful work during model loading, while retaining full validation. A read-only [phase profile](benchmarks/startup-profile-20261023.json) called the original evidence verifier, including owner/expiry/model/backend/environment/hardware/runtime binding checks: full verification 12.435 s, backend fingerprint 10.780 s, model integrity 1.597 s, metadata 0.003 s, hardware 0.048 s. CLI import median across three trials was 0.203 s versus 0.196 s for the daemon; replacing the CLI would offer little benefit. `scripts/profile-startup.py` instruments only its diagnostic process and never installs wrappers or approval caches in the running engine. No upstream runtime, installation, model, evidence or matrix changed.

The implementation prepares the existing counter after the qualified daemon exposes `control_ready`, overlapping tokenizer loading with model loading. It sends no synthetic input and changes no real-request token semantics. First polling observations are available in the shared Rust `startup` state. A changed/dead worker still latches closed until stop/restart. This shifts tokenizer initialization earlier: while the qualified engine is selected, its one CPU-only counter can consume memory before the first chat. It does not introduce a second model or duplicate counters.

Matched shared Rust headless debug builds, port 18084, same M4 / 32 GiB / Gemma 2 2B 4-bit / context 1024 / concurrency one / evidence/settings:

| Measurement | Before: lazy tokenizer | After: prepared during model load |
| --- | ---: | ---: |
| First streamed content after engine readiness | 1.225 s | 0.891 s (27.3% lower) |
| First streamed response complete | 1.305 s | 0.970 s |
| Engine start to first content | 19.069 s | 26.546 s |
| Warm complete-response median | 0.834 s | 1.204 s |
| Sampled counter RSS after requests | 200.72 MiB | 165.69 MiB |

The first-content pair is a single observation per version, with OS caches retained under ordinary desktop load. Engine-start-to-first-content and warm inference were slower in this pair; neither full startup nor warm inference speedup is claimed. RSS samples are not peak, unique physical pages, or evidence of a new memory optimization. The counter implementation/retained-tokenizer scope is unchanged; preparation now allocates it earlier. After: control observed at 21.932 s, counter spawned at 21.932 s, inference ready at 25.593 s, giving 3.661 s of model-loading overlap. These polling times include different upstream work and should not be equated with the separately measured fingerprint duration.

Both runs produced the exact initial stream answer and 24 exact EN/JA/ZH retrieval/multi-turn answers, plus six identical context refusals; prompt/completion/total usage matched for all accepted pairs. Exact context capacity is accepted and one extra token rejected. Startup observations confirm zero counters before `control_ready`; the after run confirms exactly one counter before inference readiness. Killing it causes two latched 503 responses without respawn; stop/start recovers. Stop, Strata-mock switching and SIGINT reap the full tree; ordinary history/settings/evidence bytes are preserved. Strata switching is lifecycle coverage using the mock. Verification: 69 Rust tests, including no-input preparation/same-worker reuse/dead-worker latching/restart, plus upstream compatibility/evidence/fingerprint and HTTP benchmark regression suites.

See [comparison](evaluation/prewarm-comparison-20261023.json), [before](benchmarks/prewarm-before-20261023.json), [after](benchmarks/prewarm-after-20261023.json). Freeze the pre-change `studio-server` binary before rebuilding, run the pair sequentially, and use fresh output filenames:

```sh
SOLITON_FIRST_STREAM=1 SOLITON_BENCHMARK_RUNTIME=headless node scripts/token-count-benchmark.mjs before /tmp/soliton-before-prewarm /tmp/prewarm-before.json
cargo build -p studio-server
SOLITON_EXPECT_PREWARM=1 SOLITON_FIRST_STREAM=1 SOLITON_BENCHMARK_RUNTIME=headless node scripts/token-count-benchmark.mjs after target/debug/studio-server /tmp/prewarm-after.json
node scripts/compare-token-count.mjs /tmp/prewarm-before.json /tmp/prewarm-after.json /tmp/prewarm-comparison.json prewarm
vLLM-apple/.venv/bin/python scripts/profile-startup.py vLLM-apple vLLM-apple/models/gemma-2-2b-it-4bit /opt/homebrew/opt/vllm-metal/libexec/bin/mlx_lm.server .soliton-runtime/gemma2-evidence-2026-10-22.json /tmp/startup-profile.json
```

日本語: 互換性・証跡検査後、モデル読込中に計数を先行起動しました。初回文字到着は1.23→0.89秒でしたが、起動全体とウォーム推論は今回遅く、全体の高速化は未確認です。正確な計数、上限境界、異常時拒否、エンジン排他稼働、回収を確認しました。初回チャット前からtokenizerのメモリを保持します。指紋計算は約10.8秒かかり、毎回の完全検査を残しています。起動段階はRust共有状態から観測できます。

简体中文：在兼容性及证据检查通过后，于模型加载期间提前启动计数。首个流式内容1.23→0.89秒；本次完整启动及热态推理较慢，未证实整体加速。已验证准确计数、上下文边界、异常拒绝、引擎互斥及进程回收。首次聊天前就会保留tokenizer内存。后端指纹计算约10.8秒，每次完整检查保持启用，可通过Rust共享状态观察启动阶段。

## Full-content fingerprint optimization — 2026-10-23

**Fresh full qualification and Soliton inference passed.** The measured implementation changes path handling only. Every distribution, launcher/interpreter and inventoried non-bytecode file is still read in full on every fingerprint. Per-file open/fstat/size/mtime/ctime checks, byte/file limits, `.pth` restrictions, editable/shared-environment rejection, external-link/directory-link rejection, isolated interpreter probing and the final inventory recheck remain enabled. No content, path-resolution or approval cache was added.

A profile observed approximately 124,000 strict path resolutions and 82,000 containment checks. macOS now uses the operating system's strict `realpath` through the Python standard library's ctypes binding, freeing each returned allocation; unavailable native resolution/other platforms retain pathlib. Resolution errors propagate rather than falling back to a previously valid path. POSIX containment uses normalized path text plus a separator boundary, so `env-other` cannot be treated as a child of `env`. Native canonical spelling may differ for case-aliased paths; fingerprint equivalence is verified for this measured installation rather than asserted for all possible names/platforms.

[Three paired full-content trials](benchmarks/fingerprint-content-20261023.json), the same M4 / 32 GiB and backend environment, produced the same 64-character digest on all six scans:

| Component | Original | Updated |
| --- | ---: | ---: |
| Median complete backend fingerprint | 9.353 s | 6.219 s |
| Reduction | — | 33.5% |

OS caches were retained and real-model qualification was active during these CPU trials. They are not isolated throughput certification or an end-to-end engine startup comparison. The pre-change engine attempt was correctly refused because the previous evidence's environment identity did not match the current environment; the failed [startup report](benchmarks/fingerprint-startup-before-20261023.json) is not passing inference/timing evidence. Model/backend/hardware/runtime identities matched before modification; the differing field was `environment_sha256`. The code update additionally requires a new runtime identity, so the old approval is not reused or edited.

Fresh [qualification](evaluation/soliton-vllm-apple-fingerprint-2026-10-23.json) passed **1800.076 s, 5223/5223 successes, zero failures**, 12,140,544 bytes (11.58 MiB) peak RSS growth, clean shutdown and matching before/after identity. Promotion/stream repeatability, EN/JA/ZH quality, context reevaluation and three phase samples passed. The installed backend, model and hardware digests are unchanged; environment and wrapper-runtime bindings are newly qualified. Issued 2026-10-23 20:50:58 JST, valid through 2026-10-30 20:50:58 JST if identity and workload bounds stay unchanged. No prior evidence fields were edited.

The isolated shared Rust [gateway run](benchmarks/fingerprint-startup-after-20261023.json) passed initial streamed output, 24 exact EN/JA/ZH answers, six overflows, exact-limit acceptance/one-token-over rejection, counter prewarm/failure latch/restart, stop/full-tree cleanup, Strata-mock switching and SIGINT cleanup. Ordinary state/evidence bytes stayed unchanged during that run. After validation, native and standalone saved settings were backed up and atomically changed **only** in `vllm.evidence`; documents and all other preferences compare equal. A subsequent [complete verifier profile](benchmarks/fingerprint-qualified-profile-20261023.json) passed: fingerprint 4.786 s and full evidence verification 6.229 s. These separate observations are not a controlled full-startup speedup claim. The after run observed control readiness at 10.097 s, inference readiness at 13.390 s and first content 0.706 s after readiness. Its failed pre-change counterpart remains excluded. See [combined evidence](evaluation/fingerprint-optimization-20261023.json).

Regression verification passed 35 upstream compatibility/evidence/fingerprint tests, including missing/looping/retargeted paths, non-native fallback, sibling-prefix/external-directory rejection, content writes with restored mtime and additions during hashing. Soliton's 69 Rust tests passed. The [patch](../patches/vllm-apple-fingerprint.patch) and [base/apply instructions](../patches/README.md) preserve the independently managed checkout changes; applying it to a frozen clean base reproduced both modified files exactly. The reference worker is measurement-only and is not selected by the application.

Reproduce with the selected backend's adjacent Python; preserve its interpreter symlink so the virtual environment is not discarded:

```sh
python3 scripts/benchmark-fingerprint.py --python /opt/homebrew/opt/vllm-metal/libexec/bin/python --executable /opt/homebrew/opt/vllm-metal/libexec/bin/mlx_lm.server --before scripts/fixtures/fingerprint-reference.py --after vLLM-apple/vllm_apple/backend_fingerprint.py --output /tmp/fingerprint-comparison.json
```

日本語: 全ファイルを毎回読み直す指紋計算を、パス処理の改善で9.35→6.22秒（33.5%短縮）にしました。6回すべてのSHA-256が一致し、変更検出・上限・外部パス拒否を維持します。以前の証跡は環境の不一致で正しく拒否され、変更後は新しい30分検証が5223件成功・失敗0件で合格しました。Solitonの実推論・正確な上限拒否・排他稼働・回収も通過し、通常起動の証跡設定だけを更新しました。起動全体の前後比較は主張しません。

简体中文：通过优化路径处理，每次完整读取全部文件的指纹计算9.35→6.22秒（降低33.5%）。六次SHA-256完全一致，保留变更检测、限制及外部路径拒绝。旧证据因环境不匹配被正确拒绝，修改后的新30分钟验证5223次全部成功、零失败。Soliton真实推理、准确上下文拒绝、引擎互斥及回收通过，仅更新正常启动的证据设置。不声称完整启动时间的前后加速。

## Multilingual exact context matrix — 2026-10-23

Run the existing isolated shared-Rust real-model suite with the additional matrix:

```sh
SOLITON_BENCHMARK_RUNTIME=headless SOLITON_EXPECT_PREWARM=1 \
SOLITON_FIRST_STREAM=1 SOLITON_CONTEXT_MATRIX=1 \
node scripts/token-count-benchmark.mjs after target/debug/studio-server \
  docs/benchmarks/context-matrix-NEW.json
```

English: The matrix measures the actual prompt usage first, then reserves exactly `context - prompt_tokens` output tokens. EN/JA/ZH retrievals, each with and without a synthetic earlier conversation, exercise both ordinary JSON and SSE responses. Every condition must return the exact identifier at the boundary and HTTP 422 `context_length_exceeded` with one additional reserved token. Stream runs require a normal `[DONE]` terminator and reject error frames. The fixture requires a 32–511-token output reservation; it fails explicitly outside that range instead of silently adapting an unqualified profile. Reports contain counts and timings, not prompts or conversation content. This expands coverage within the existing 1,024-token/concurrency-one evidence; it does not certify a larger context or extend evidence expiry.

日本語: 実際のprompt usageから出力予約数を算出し、英語・日本語・简体中文、履歴あり／なし、JSON／SSEの12条件で境界を検証します。上限ちょうどでは識別子の完全一致、1トークン超過では推論前の422拒否を必須とします。既存の1,024トークン・同時実行1の証跡内の検証であり、上限や有効期限は変更しません。

简体中文：根据实际prompt usage计算输出预留量，验证英日中三种语言、带历史／无历史、JSON／SSE共12种条件。恰好达到上限时必须准确回答，多预留一个token时必须返回422。验证范围仍为已有证据的1024 token、并发1，不扩大上下文，也不延长证据有效期。

Verified report: [context-matrix-verified-20261023.json](benchmarks/context-matrix-verified-20261023.json), `passed: true`: 12 boundary answers, 12 overflow refusals, six probe answers, and the original 24 answers/six refusals passed. Counter failure stays latched until explicit restart; Stop, Strata-mock switching and headless SIGINT reap owned processes. User state and evidence hashes remain unchanged. The initial [incomplete run](benchmarks/context-matrix-20261023.json) is retained as `passed: false`: its missing explicit language selection exercised English only, and the coverage assertion prevented a false pass. Neither run measures a before/after performance improvement.

## Qualified context expansion — 2026-10-24

The candidate raises the exact Gemma 2 2B 4-bit installation's total input plus requested output reservation from 1,024 to 4,096 tokens, at concurrency one. The model, backend, dependency files, runtime, environment identity and normal compatibility matrix stay unchanged. Fresh full qualification is mandatory; the old 1,024 evidence cannot authorize 4,096, and new 4,096 evidence cannot authorize 4,097. Owner-only copies of the unchanged original qualification report and actual Soliton startup refusals are retained in [qualification](evaluation/soliton-vllm-apple-context4096-20261024.json) and [startup gates](evaluation/context4096-startup-gates-20261024.json).

The extended `SOLITON_CONTEXT_MATRIX=1` runner uses a bounded search over synthetic record lengths. It uses actual response-reported token counts to select 3,968–4,064-token prompts, reserves the remainder exactly, and verifies one-token-over refusal for both JSON and SSE. Three query languages, beginning/middle/end identifier placements, single messages or six prior user/assistant pairs plus the final question (13 messages), and three repeated rounds yield 108 boundary answers and 108 expected refusals. The latest question contains no identifier in the history cases. Oversized search probes must refuse specifically for context overflow; accepted probes must answer exactly. It requires normal SSE termination, samples owned RSS and Rust engine physical footprint, and verifies bounded shared request telemetry. Fixed earlier 64-row overflow fixtures are now expected to fit at 4,096; historical 1,024 runs keep their original refusal expectations. All inference is serial through the shared Rust gateway.

Use an isolated seed with the candidate context and matching fresh evidence; this does not change normal preferences:

```sh
SOLITON_NATIVE_STATE=/absolute/path/to/private-candidate-state.json \
SOLITON_REAL_EVIDENCE=/absolute/path/to/fresh-4096-evidence.json \
SOLITON_BENCHMARK_RUNTIME=headless SOLITON_EXPECT_PREWARM=1 \
SOLITON_FIRST_STREAM=1 SOLITON_CONTEXT_MATRIX=1 \
node scripts/token-count-benchmark.mjs after target/debug/studio-server /tmp/context4096-NEW.json
npm run test:context-matrix
```

`activate-context-profile.py` is an explicit preference migration for this measured 4,096 profile. It requires the complete passing matrix, matching evidence hashes, actual Soliton refusal reports, no live preferences owner, and the original full startup verifier against each state's current model/backend/environment. It backs up original bytes and atomically changes only `settings.vllm.context` and `settings.vllm.evidence`, preserving documents and other preferences. It rejects optimized Python (`-O`) so assertions cannot be disabled. New installations retain the conservative 1,024 default and must supply their own valid evidence.

The 30-minute qualification workload is mixed short text under a 4,096 configured bound. The separate near-limit matrix tests repeated long inputs; it is not a claim of a 30-minute near-limit workload, arbitrary long-document comprehension, all model families, another Mac, larger contexts, GUI interaction at 4,096, or performance improvement. RSS snapshots and physical footprint are distinct measurements; per-request snapshots are not continuous GPU allocation peaks. The older `test:real-model` GUI suite remains a 1,024-scope fixture requiring matching 1,024 evidence; use the extended headless matrix for this expansion.


Verified result: [consolidated evidence](evaluation/context-expansion-20261024.json), [real Soliton matrix](benchmarks/context4096-matrix-20261024.json), [actual-fixture full token-ID parity](evaluation/context4096-token-parity-20261024.json). Qualification passed 1800.106 s with 5918/5918 successes, zero failures, 16 MiB peak RSS growth and clean shutdown; the complete identity equals the previous 1,024 profile. The separate long matrix passed in 598.083 s: 108/108 exact boundary answers and 108/108 expected one-token-over refusals, actual input 3,970–4,033 tokens. All 24 previous accepted prompt counts match; all six previous overflow cases now answer exactly. Eighteen actual long fixtures match the original tokenizer's full IDs, in addition to 211 raw/844 chat cases, three malformed-role rejections and six fallback guards. Node tests: 22; upstream evidence/fingerprint/compatibility tests: 35.

All 108 owned-tree memory samples succeeded: sampled RSS maximum 2,037.98 MiB, engine physical footprint maximum 7,813.60 MiB (7.63 GiB), minimum system available memory 7.76 GiB; retained counter RSS at the final sample was 33.30 MiB. These are different memory scopes, not a memory-reduction comparison or continuous allocation peaks. Shared API telemetry remains capped at 100 records with long prompt usage retained. Counter failure refuses/latches until explicit restart; Stop, Strata-mock switch and headless SIGINT remove the owned tree. Both ordinary native/headless preferences now select context 4,096 and the fresh evidence, with original-byte backups and no document/other-preference changes. Startup still revalidates in full; evidence expires 2026-10-31 12:30:03 JST while identity remains unchanged.

日本語: コンテキスト合計を1,024→4,096へ拡大しました。30分の短文混合試験は5,918件成功・失敗0件。別途、約10分の長文・13メッセージ履歴試験で、108件の完全一致と108件の1トークン超過拒否を確認しました。入力は3,970〜4,033トークンで、元のTokenizerとの全ID一致も確認しています。通常設定のcontextと証跡だけを更新し、会話・他の設定・排他稼働・互換性検査を維持しました。エンジンのphysical footprintは最大約7.63 GiBを観測しており、次は長文時のKV／prompt cacheのメモリと解放を検証します。新規環境の既定値は1,024です。

简体中文：上下文总量从1024扩展到4096。30分钟短文本混合测试5918次成功、零失败；另行约10分钟的长文本及13条消息历史测试，108次边界回答完全正确，108次多预留一个token的请求均被拒绝。实际输入3970–4033 token，全部token ID与原Tokenizer一致。只更新正常配置的context和证据，保留会话、其他设置、互斥运行和完整兼容性检查。观察到引擎physical footprint最高约7.63 GiB，下一步测量长历史的KV／prompt cache内存和回收。新环境默认值仍为1024。

## Diverse long-history cache memory — 2026-10-24

The opt-in `SOLITON_CACHE_MEMORY=1` workload uses the existing isolated shared-Rust real-model runner. It keeps context 4,096, concurrency one, complete startup evidence verification and per-request exact CPU counting. Sixteen deterministic early prefixes cover EN/JA/ZH queries and six historical user/assistant pairs plus a final question. The final question contains no identifier. Each response must return the exact identifier; repeated instances must report identical input counts. Missing `prompt_tokens_details.cached_tokens` fails rather than becoming zero.

The protocol first measures a cold anchor and immediate reuse, checks exact total 4,096 acceptance / 4,097 refusal, then runs two rounds of fifteen other histories followed by anchor revisit/reheat and recent-history reuse. Reuse loss after churn is consistent with eviction under the installed default ten-entry cache, not direct cache-inventory proof. All inference stays serial. The runner samples owned RSS, engine physical footprint and system available-memory estimates after each request, then at 0/5/10 idle seconds; low available memory stops the test rather than forcing OS pressure. Stop must remove the owned tree and clear the native engine footprint. Shared cache-usage telemetry, counter-failure latching/recovery, Strata-mock switching and SIGINT cleanup remain tested. It does not certify byte-level KV accounting, continuous GPU allocation peaks, a memory cap, forced-pressure behavior, or memory/performance reduction.

```sh
SOLITON_REAL_EVIDENCE=/absolute/path/to/fresh-matching-evidence.json \
SOLITON_BENCHMARK_RUNTIME=headless SOLITON_EXPECT_PREWARM=1 \
SOLITON_FIRST_STREAM=1 SOLITON_CACHE_MEMORY=1 \
node scripts/token-count-benchmark.mjs after target/debug/studio-server /tmp/cache-memory-NEW.json
npm run test:cache-memory
```

The initial [refused run](benchmarks/cache-memory-20261024.json) has `passed: false` and no inference cases: current login/tool environments include `PYTORCH_ENABLE_MPS_FALLBACK=1`, while the previous proof bound an empty set of relevant flags. Model/backend/runtime identity is unchanged. The flag was not removed to reuse old approval; fresh full qualification is required for this environment. The benchmark now fails promptly when its supervised engine exits before readiness, instead of polling until the ready timeout. Failed startup is not a memory baseline.

日本語: 16種類の長文履歴を直列で2巡し、再利用・追い出し後の再計算・再加熱・10秒アイドル後の保持量を測ります。件数とバイトの上限、実際のKV使用量、RSS、physical footprintは別の値です。再利用トークン数が減っただけで「物理メモリを解放した」とは判断しません。通常の正確な事前計数・完全な証跡検査・排他稼働を維持し、会話を含む通常設定にはテストデータを書きません。

简体中文：串行执行两轮、16种长历史，测量复用、轮换后的重新计算、再次复用及空闲10秒后的内存。条目上限、字节上限、KV用量、RSS和physical footprint是不同指标；复用token减少不代表物理内存已释放。保留准确预先计数、完整证据检查和引擎互斥，不把测试内容写入正常会话配置。


Verified: [summary](evaluation/cache-memory-20261024.json), [real workload](benchmarks/cache-memory-verified-20261024.json), [full-ID parity](evaluation/cache-memory-token-parity-20261024.json), [environment-only change](evaluation/cache-memory-identity-20261024.json), [startup bounds](evaluation/cache-memory-startup-gates-20261024.json), [fresh full qualification](evaluation/soliton-vllm-apple-cache-memory-env-20261024.json). The fresh current-environment report passed 1800.675 seconds, 5978/5978 requests, zero failures, 15.27 MiB peak RSS growth and clean shutdown. Earlier proof remains unchanged and is correctly rejected in the changed environment. The [fast refusal](benchmarks/cache-memory-fast-refusal-20261024.json) completed in about 11.2 seconds with zero inference cases, preserving `passed: false`.

The separate cache workload passed in 375.261 seconds: 38/38 exact answers, 3,930–3,937 input tokens, 16 histories, two anchor reuse losses after churn and successful reheat/recent-history reuse. Exact total 4,096 was accepted and 4,097 refused. Full token-ID parity passed for every actual fixture, plus 211 raw/844 chat cases, three malformed-role rejections and six fallback guards. The common Rust request log retained 70 API records and correctly reported cached-token values 4–3,934. Counter failure stayed latched until explicit restart; Stop, Strata-mock switching, recovery and SIGINT cleanup passed. Tests: 26 Node, two profile-refresh rejection tests, 35 upstream evidence/fingerprint/compatibility tests.

| Observed phase | Anchor reused tokens | Engine physical footprint |
| --- | ---: | ---: |
| Before long-history workload (already warmed by short cases) | — | 2.82 GiB |
| First anchor | 4 / 3,935 | 3.63 GiB |
| Immediate anchor reuse | 3,934 / 3,935 | 4.59 GiB |
| End of first churn/revisit/reheat cycle | recent history: 3,934 / 3,935 | 6.54 GiB |
| End of second cycle | recent history: 3,934 / 3,935 | 7.96 GiB |
| Ten seconds idle | no inference | 7.91 GiB |
| Stop | no model/counter processes | no engine footprint sample |

Both anchor revisits reused only eight tokens, then reheated to 3,934. Cache misses after churn and memory retention are different observations; this does not prove exact cache inventory, leaked allocations, or which bytes belong to retained KV versus MLX allocator cache. Sampled owned-tree RSS reached 2,423.44 MiB; system available-memory estimate stayed at least 6.62 GiB. Native physical footprint includes memory outside RSS; neither is unique KV bytes. No forced-pressure test or before/after memory reduction is claimed. The next intervention is a measured byte budget, with allocator policy assessed separately and a fresh certification for any runtime/environment change.

Ordinary native/headless preferences now use `.soliton-runtime/gemma2-cache-memory-env-20261024.json`; only the evidence path changed, with byte-for-byte original backups and unchanged context 4,096, documents and other settings. Evidence expires 2026-10-31 18:31:45 JST if the measured environment/model/backend/runtime/Mac stays identical. The activation tool's cache-workload path is restricted to refreshing an already-certified 4,096 profile when only environment identity changes; raising context or changing any other identity requires the complete long-context matrix. It still runs the original full startup verifier for each state before writing.

日本語: 38件の長文要求はすべて正答し、2巡とも再利用数が3,934→8→3,934へ変化しました。ただしphysical footprintは最大7.96 GiB、10秒アイドル後も7.91 GiBを保持しており、追い出しだけでは物理メモリの解放を意味しません。停止後は所有プロセスとエンジン計測が消えました。現在の環境変数を含む30分再認証は5,978件成功・失敗0件で合格し、通常設定は証跡パスだけ更新、コンテキスト4,096・会話・他の設定・排他稼働を維持しました。次は保持KVのバイト上限とアロケータを分けて検証します。

简体中文：38次长文本请求全部准确，两轮复用数均为3934→8→3934。但physical footprint最高7.96 GiB，空闲10秒后仍保持7.91 GiB；复用下降不表示物理内存已经释放。停止后所属进程和引擎内存样本均消失。包含当前环境变量的完整30分钟再认证5978次成功、零失败。正常配置只更新证据路径，保留4096上下文、会话、其他设置和引擎互斥。下一步分别验证保留KV的字节上限和分配器缓存。


## Retained KV budget — 2026-10-25

The measured M4 / Gemma 2 2B IT 4-bit profile now uses `kvCacheMiB: 1024`, total context 4,096 and concurrency one. New settings default to zero (original backend policy); other budgets/models require matching fresh qualification. [Consolidated report](evaluation/kv-budget-20261025.json), [raw after workload](benchmarks/kv-budget-after-20261025.json), [full context matrix](benchmarks/kv-budget-context-20261025.json), [startup gates](evaluation/kv-budget-gates-20261025.json).

| Matched primary workload | Before, 2026-10-24 | After, 2026-10-25 |
| --- | ---: | ---: |
| Exact answers / histories | 38 / 16 | 38 / 16 |
| Engine physical footprint peak | 7.961 GiB | 4.582 GiB |
| Physical footprint after 10 idle seconds | 7.914 GiB | 3.303 GiB |
| Primary workload duration | 375.261 s | 438.617 s |
| Recent-history cached tokens, both rounds | 3,934 | 7 |
| Recent-history response time | 0.474 / 0.451 s | 13.180 / 10.866 s |

Accounting clarification (2026-10-25): the duration row includes three additional seeded requests only in the after run. It is not a matched latency comparison; the physical-memory comparison uses the matched 38 requests / 42 primary samples. The allocator comparison below includes the same three seeded requests in both runs.

The observed peak decreases 42.4% and idle footprint 58.3%. This is a temporal comparison on a loaded desktop, not controlled latency evidence. The valid baseline is [the previous passing run](benchmarks/cache-memory-verified-20261024.json). Today's [baseline attempt](benchmarks/kv-budget-before-20261025.json) failed because one transient descendant exited between enumeration and RSS sampling; it is excluded. Its binary, evidence and settings hashes exactly equal the passing baseline. The sampler now retries unstable/missing membership and still fails persistent errors rather than substituting zero. Reuse losses are expected with the smaller capacity: exact answers/counts remain mandatory; recent-hit assertions apply only to the original policy. Main anchor reheat still requires reuse.

An official `--prompt-cache-bytes` flag alone trims before batched work and does not bound completed retention on the seeded sequential path in this installed version. An initial qualification was interrupted for that reason and issued no evidence. The final adjacent-Python isolated wrapper checks MLX-LM 0.32.0 and exact server/cache source SHA-256, sets the original LRU's `max_bytes`, and synchronizes original insertion/eviction/fetch operations. It changes no installed files or attention/sampling/tokenization logic. Source or dependency changes fail closed. The independent checkout patch is in [patches](../patches/README.md).

The separate read-only scalar endpoint samples retained logical LRU bytes, entry count and non-atomic MLX allocator active/cache/peak values. Across 153 samples (108 matrix, 42 primary cache and three extra seeded requests), retained KV never exceeds 1,024 MiB; maximum 1011.461 MiB. Logical tensor bytes are not unique physical allocation accounting. Active KV, weights, allocator cache, backing allocations and temporary buffers remain separate: this is not a 1 GiB total-process cap. The raw workload's legacy `observations` sentence says no inventory; the actual `kvCache` sample fields and this report supersede that sentence. Forced OS pressure remains untested. The real installed LRU also passes a five-insertion logical-byte fixture (2,000→800 MiB), which allocates no model weights and is not a RAM benchmark.

Full fresh qualification: 1800.298 seconds, 5,505/5,505 successes, zero failures, 18.203 MiB peak RSS growth, clean shutdown. Identity is bound to the changed runtime plus `PYTORCH_ENABLE_MPS_FALLBACK=1` and `VLLM_APPLE_KV_CACHE_MIB=1024`; original full checks run at activation and every startup. Evidence created 2026-10-25 15:29:12 JST; seven-day expiry 2026-11-01 15:29:12 JST.

Real shared Rust API validation: 108 exact-limit answers and 108 one-token-over refusals, 3,970–4,033 input tokens; EN/JA/ZH × single/13-message history × start/middle/end × JSON/SSE × three rounds. Additional 38 exact diverse-history answers plus three seeded exact answers exercise both backend paths. Complete token-ID parity passes 211 raw, 844 chat and all 16 actual long-history fixtures; three invalid conversations reject and six fallback guards pass. Wrong budgets (zero/512) reject by evidence identity; 4,097 rejects by certified limit before model/counter launch. Counter failure latch/recovery, bounded shared telemetry, Strata-mock exclusive switching and whole-tree stop/signal cleanup pass. Preferences activation changes only evidence and KV budget in native/headless documents, with backups and original full verification; history and all other preferences match.

Validation: 70 Rust tests, 64 backend/compatibility/fingerprint/evidence tests, 30 Node workload/sampler tests and two activation refresh guards; production UI and unsigned native app rebuilt. Playwright/Chrome fixture QA checks visible Soliton identity, nonblank page, no blocking overlay, 320/390/1,440 px, EN/JA/ZH, budget edit/disabled states/persistence/reset, switching and restart recovery. Only expected unauthorized/connection messages during deliberate private-server restart are allowed; successful authenticated reconnect is required. Browser plugin skill/runtime was unavailable, so the repository Playwright fallback was used. Actual model inference uses shared Rust API; native GUI GPU inference is not newly certified.

日本語: 保持KVを1 GiBに制限し、物理メモリの観測値は減少しました。ただし一部履歴の再計算で応答が遅くなります。正確な計数・互換性検査・排他稼働は維持し、実モデルの長文・履歴・境界・seed指定経路を検証しました。これは総メモリ上限ではありません。

简体中文：保留KV限制为1 GiB，观测到的物理内存下降；部分历史需要重新计算，响应变慢。准确计数、兼容性检查和引擎互斥保持不变，已用真实模型验证长文本、历史、边界及指定seed路径。这不是总内存上限。


## Allocator free-cache budget — 2026-10-25

The measured M4 / 32 GiB / Gemma 2 2B IT 4-bit profile now uses `allocatorCacheMiB: 256`, alongside retained KV 1,024 MiB, total context 4,096 and concurrency one. Generic default zero preserves the original allocator policy; other values require matching fresh evidence. [Consolidated report](evaluation/allocator-20261025.json), [before](benchmarks/allocator-before-20261025.json), [after](benchmarks/allocator-after-20261025.json), [context matrix](benchmarks/allocator-context-20261025.json), [startup gates](evaluation/allocator-gates-20261025.json).

| Same-day matched workload | Before: backend default | After: 256 MiB free cache |
| --- | ---: | ---: |
| Exact history answers / extra seeded answers | 38 / 3 | 38 / 3 |
| Physical footprint peak, 42 primary samples | 4.582 GiB | 3.630 GiB |
| Physical footprint peak, all 45 cache samples | 4.582 GiB | 4.188 GiB |
| Physical footprint after 10 idle seconds | 3.313 GiB | 3.316 GiB |
| Allocator free-cache peak, all 45 samples | 1853.923 MiB | 259.599 MiB |
| First stream content, after engine ready | 642.601 ms | 274.817 ms |
| Engine start to first stream content | 15.222 s | 14.032 s |
| Short-request warm median, 29 requests | 897.387 ms | 1131.755 ms |
| Long-history HTTP median, primary 38 | 10.045 s | 12.265 s |
| Long-history HTTP sum, primary 38 | 367.446 s | 451.121 s |
| Entire cache workload, including checks/waits/samples | 415.702 s | 513.046 s |

Host one-minute load was 2.16→2.80 before and 4.58→11.07 after. Warm short/long medians are slower in this pair, first stream is faster, and idle physical footprint is essentially unchanged. The measurements do not isolate the cause of these timing changes.

Both runs use the same 38 history requests, three seeded requests, two boundary checks (one acceptance/one refusal), idle waits and native measurement protocol. Their prompt counts match exactly. Request latency excludes memory-sampling overhead; whole-workload duration includes it and the waits. The physical peak across all 153 after-validation samples (matrix plus cache/seeded) is 4.188 GiB, a separate cohort from the paired cache comparison. Timings are a single sequential same-day observation on a loaded desktop; they do not establish a controlled statistical speedup. All measured changes, including regressions, are retained in the table and raw files.

The installed MLX `set_cache_limit` API is called once before weights load, retaining a stricter existing limit. No new per-request flush, attention/sampling/template/token change or second resident model is introduced. Full installed-engine fingerprints and exact reviewed server/cache source guards remain in force. The policy limits reusable free allocations; active weights/KV/temporaries, GPU-driver backing allocations and process physical RAM remain separate. MLX reclaims excess on later allocation, so observed free-cache bytes can momentarily exceed the nominal limit. Configured/effective limits and non-atomic active/cache/peak values are reported separately. Retained logical KV stays within 1,024 MiB across 153 samples, maximum 1011.461 MiB. Periodic sampling can miss brief peaks; no forced OS-pressure test or unique physical KV ownership claim is made.

Fresh full qualification passed 1800.094 seconds, 4,186/4,186 successes, zero failures, 11.203 MiB peak RSS growth, clean shutdown and identical before/after identity. Installed engine/model/hardware identities match the prior profile; runtime and environment identity intentionally change. Explicit flags are `PYTORCH_ENABLE_MPS_FALLBACK=1`, `VLLM_APPLE_KV_CACHE_MIB=1024`, `VLLM_APPLE_ALLOCATOR_CACHE_MIB=256`. Evidence created 2026-10-25 17:44:04 JST, expires 2026-11-01 17:44:04 JST. [Raw certificate](evaluation/soliton-vllm-apple-allocator-20261025.json).

Real Soliton shared Rust API validation passes 108 exact-limit answers / 108 one-token-over refusals (3,970–4,033 input tokens), EN/JA/ZH × single/13-message history × start/middle/end × JSON/SSE × three rounds. All 38 history and three seeded answers are exact. Complete token-ID parity passes 211 raw + 844 chat + 16 actual long-history fixtures, three invalid-conversation refusals and six fallback guards. Allocator zero/128 with the new evidence refuse by identity; allocator 256/context 4,097 refuses by scope, with no inference/counter launch. Failure latch/recovery, exclusive Strata-mock switch, bounded shared telemetry, full owned-tree cleanup, unchanged documents/evidence and restart pass. [Token parity](evaluation/allocator-token-parity-20261025.json).

The guarded activation consumes the full matrix, matched measurements (requiring lower measured physical/cache peaks), 153 policy samples and startup gates, then calls the original complete startup verifier for each native/headless preference file. Only allocator budget/evidence change; context, KV budget, documents, history and other preferences match original-byte backups. Startup still performs full verification. Reproducible changes in the independent backend checkout are retained as an [ordered patch](../patches/README.md).

Regression validation: 71 Rust, 69 backend/evidence/fingerprint, 30 Node workload/sampler and two activation refresh-guard tests pass; production UI and unsigned native app rebuilt. Chrome/repository Playwright fixture QA at localhost:18080 covers visible Soliton identity, nonblank page, no overlay/unexpected console error, EN/JA/ZH at 320/390/1,440 px, allocator edit/persistence/reset, engine lifecycle and authenticated restart recovery. Browser skill was unavailable, so the existing Playwright workflow was used. Real model testing uses the shared Rust API; native GUI GPU inference is not newly certified. Screenshot evidence remains outside the repository at `/tmp/soliton-allocator-policy-control.png` and `/tmp/soliton-allocator-policy-日本語.png`.

日本語: 空きアロケータの保持上限を256 MiBに設定し、メモリと応答時間の観測値を上表へ記録しました。正確な計数・互換性検査・排他稼働は維持し、既存履歴も保持しています。総RAM上限や統計的な速度向上を保証するものではありません。

简体中文：空闲分配器缓存上限设为256 MiB，内存和响应时间观测值记录于上表。准确计数、兼容性检查、引擎互斥及现有历史保持不变；这不是总RAM上限，也不代表统计上已证实的加速。


## Intermediate allocator comparison protocol — 2026-10-28

The current experiment compares intermediate allocator budgets against the 256 MiB profile without changing retained KV (1,024 MiB), exact admission, total context (4,096), concurrency (one), model/weights, attention or sampling. The existing startup evidence is correctly refused after runtime and OS identity changes (current measured host: Apple M4 / 32 GiB / macOS 27.2); old proof is preserved, never rewritten.

`scripts/allocator-budget-trial.py` uses the original qualification-only exact candidate-version check, model/capability/memory/context checks, streaming/sampling promotion, three phase samples, multilingual quality smoke and a short non-certifying soak. Those reports contain no architecture-evidence binding and cannot authorize ordinary startup. Each actual comparison then runs a fresh original supervised backend, with the original complete model/backend/hardware/runtime/environment identity captured before/after. Every request uses the existing CPU tokenizer worker, verifies local/backend exact prompt counts and the exact answer. Sixty-five requests per trial comprise six priming requests (excluded from warm statistics), 18 warm-before, 20 diverse long-history requests, three seeded requests and 18 warm-after. Twenty-six samples measure owned-backend physical footprint/RSS through the same public Darwin `ri_phys_footprint` field as Rust, scalar logical KV bytes, separate allocator counters and idle retention. The backend-only tree excludes the Soliton control process; do not mix its absolute footprint/latency with shared Rust API measurements.

An unrelated host-load episode overlapped Xcode/Simulator startup during initial trials. Functional results and raw samples are retained, but entire contaminated trials are excluded from latency comparisons. The revised runner waits for three consecutive 20-second readings at one-minute load ≤6 and rejects trials with load >10. No unrelated processes are stopped. Each candidate has two repeats bracketed by 256 MiB controls and backend-default references; comparison replays the exact workload/count/cache-reuse signatures and rejects non-environment identity changes. Selection requires both warm-short and long-miss medians lower than the paired 256 controls (at least one by 5%), primary physical peak at least 5% below the default reference, all-path/seeded physical peak still below that reference, and at least 25% lower free-cache peak. Primary and seeded/all-path peaks are reported separately. These are engineering selection rules for observed runs, not randomized statistical proof or a total-RAM limit.

Adoption requires a fresh unchanged-source full ≥1,800-second qualification at the selected explicit environment, followed by the actual Soliton shared Rust API history/seeded workload, full 108-case multilingual context matrix/108 overflow refusals, failure latch/recovery, exclusive switching, owned-tree cleanup and wrong-budget/over-context startup gates. Guarded activation re-hashes raw pilots, recomputes selection, matches selected pilot identity to the full certificate and calls the original full startup verifier before changing only budget/evidence preferences. Documents/history and all other preferences must match the backups. Final measurements and adoption status follow below when complete.

日本語: 予算ごとに同じ実モデル・入力・正確な計数で比較します。高負荷と重なった試行は生データを残して速度比較から除外します。短い比較結果は起動承認に使わず、採用候補に完全な30分認証とSoliton本体の長文・境界・排他試験を実施します。通常経路とseed指定経路のメモリを区別し、速度だけが改善する設定は採用しません。

简体中文：每个预算使用相同真实模型、输入及准确计数。与主机高负载重叠的试验保留原始数据，但不用于延迟比较。短期比较不授权正常启动；候选配置仍需完整30分钟认证及Soliton长文本、边界、互斥测试。分别报告普通与指定seed路径的内存，不采用仅速度改善而丢失内存收益的配置。


### Parallel-work protocol revision and current status

The initial 65-request/26-sample experiments above proved functional but could not establish a clean latency comparison during repeated Xcode/Simulator load episodes. Their raw records remain immutable; they are not combined with the revised micro cohort. The revised symmetric order is backend-default → 256 → 512 → 768 → 768 → 512 → 256 → backend-default. Each fresh micro trial contains 52 exact-count requests (six excluded priming requests, 36 warm-short requests, seven long-history requests with four genuine long misses, and three seeded requests) and 13 native/scalar samples. It uses the same 3,930–3,937-token histories across EN/JA/ZH, with retained KV 1 GiB and context 4,096 unchanged. Full 16-history/38-request + three-seeded and 108-boundary Soliton validation is still required after complete qualification.

One-minute load averages lag active work, so the new observer uses public Darwin CPU counters and owned-process CPU times, converting Mach absolute ticks with the actual timebase and checking the units against the process CPU clock. It separates inference's own CPU work from background activity; no unrelated process is stopped. Before a trial it requires three five-second background readings ≤40%; before measured requests it waits for background activity to settle. Request-average background CPU must be ≤50%, with no sustained ≥75% interval of one second. Peaks and wait times are recorded rather than hidden; resource waits are outside request latency, so this is not an end-to-end throughput test. Missing/stale/unchanged counter intervals are never treated as free CPU. Short kernel snapshot intervals are awaited after the response timer, and priming data is excluded from latency statistics. Source/fixture/observer hashes must match across complete trials; resumption never overwrites earlier raw data.

As of the 2026-10-28 work session, repeated background activity still prevented a complete eligible comparison. No intermediate setting, full certificate, or production speedup has been issued. Native/headless preferences remain at 256 MiB and existing history/settings are unchanged. The prior certificate is preserved and correctly refuses the changed runtime/OS identity; it cannot be refreshed by modifying its body or removing flags. Next steps are a valid repeated comparison, an eligible measured selection, original full 30-minute certification, common Rust API validation and complete guarded startup verification before changing preferences.

日本語: 並行作業を止めず、短い同条件の比較で有効な測定を待っています。過負荷・途中失敗のデータを速度改善の根拠には使わず、新しい設定は未採用です。現行の256 MiB設定を保持し、変更した環境には完全な再認証が必要です。

简体中文：保留并行工作，通过相同条件的短周期试验等待有效测量。高负载或中断数据不用于声称加速，尚未采用新预算。保留256 MiB配置；环境变化后仍需完整重新认证。


## Intermediate-budget pilot result — 2026-10-28

All eight valid symmetric micro trials are complete: backend default,256,512,768,768,512,256,default. Each passed52 exact answers and13 native samples; source/fixture/observer hashes, token/cache signatures, identity and live background-load guards match. Failed or overloaded attempts remain preserved and excluded. The saved [comparison](evaluation/allocator-tuning-comparison-20261028.json) was recomputed from its immutable raw files and hash-checked.

| Budget MiB | Warm short ms | Long miss ms | Primary physical peak GiB | All-path physical peak GiB | Free-cache peak MiB |
|---|---:|---:|---:|---:|---:|
| 0 | 393.553 | 8737.001 | 4.183 | 4.498 | 1683.921 |
| 256 | 383.907 | 8861.579 | 3.609 | 4.075 | 261.276 |
| 512 | 380.764 | 7754.717 | 3.608 | 4.229 | 516.978 |
| 768 | 390.562 | 8727.191 | 3.676 | 4.493 | 774.883 |

512 MiB meets the predeclared selection guards: versus256, short median decreases0.82% and long median12.49%; versus default, primary physical peak decreases13.76%, all-path peak5.99%, free-cache peak69.30%. Its all-path peak remains higher than256; this is a tradeoff, not an improvement in every metric. 768 MiB fails because short median increases1.73%. Timing aggregates are geometric means of two per-run medians; memory values are maximum observed peaks.

The short difference is small and may reflect residual host/thermal variability below the guard. These are Python exact-counter plus direct-backend timings, not common Rust API latency or statistical certification. No throughput/global-RAM-cap claim is made. Prior statements that the cohort was incomplete describe earlier checkpoints. Original1800-second qualification of512 has started; common Rust API integration, boundary/fault/lifecycle gates and guarded preference activation remain unfinished. Preferences stay256 with the preserved old identity-mismatched proof refused.

日本語: 有効8試行が揃い、512 MiBが事前の選択条件を満たしました。短文差は小さく、Rust共通APIの速度改善は未検証です。完全認証中で設定は未変更です。

简体中文：8次有效试验已完成，512 MiB满足预设选择条件。短文本差异很小，Rust公共API的加速尚未验证。完整认证正在进行，配置保持不变。


## 512 MiB adoption — 2026-10-28

Original full qualification passed1,800.235 seconds,5,863 successes/zero failures with clean shutdown and fresh identity-bound evidence. Common Rust API validation passed38 long-history requests and three seeded requests;108 exact-limit answers and108 one-token-over refusals across EN/JA/ZH, single/history, position, JSON/SSE and three rounds. Counter failure closed/latched and recovered; stop/signal reaped model and counter; switching to the existing Strata mock kept exclusive ownership. This does not certify simultaneous real Strata inference. Wrong allocator0/256 refused by identity, and512/context4097 refused by evidence limits, without inference/counter startup or history mutation.

The original activation guard replayed all eight pilot files, verified hashes/selected identity, checked153 retained-KV/allocator samples and ran full startup evidence verification for both preference documents. Native/headless now select allocator512 MiB, retained KV1024 MiB/context4096/concurrency1. Original-byte backups were saved; only allocator budget/evidence changed, preserving conversations/history and other preferences. Old proofs were not edited and no compatibility/count/exclusivity guard was bypassed. [Consolidated report](evaluation/allocator-tuning-20261028.json).

Evidence created 2026-10-28 14:19:43 JST, expires 2026-11-04 14:19:43 JST. Maximum sampled retained KV 1011.461 MiB across153 samples.

Python pilot improvements above remain separate from common Rust integration timings: the latter validate functionality under continuing desktop work and do not establish another causal speedup. Limits apply to reusable allocator cache, not active allocations or total RAM.

日本語: 完全認証・長文履歴・境界判定・計数異常復旧・起動拒否・排他切替の検証を通過し、512 MiBを採用しました。会話と他の設定は保持しました。速度比較はPython経路、Soliton検証はRust共通APIで、数値は混同しません。

简体中文：完整认证、长历史、边界、计数故障恢复、启动拒绝及排他切换均通过，已采用512 MiB。保留会话和其他设置。Python性能比较与Rust公共API验证分别报告。


## Active/backing retention investigation — 2026-10-28

With allocator512 MiB/retained KV1024 MiB/context4096, a new qualification-only current-runtime diagnostic passed52 exact answers and13 native samples. Observed physical peak4338.800 MiB, MLX active peak3049.821 MiB; idle physical3192.784 MiB, active2167.430 MiB, logical retained KV611 MiB and free allocator cache365.760 MiB. Active/logical-KV/free-cache observations are non-atomic and not disjoint physical ownership; do not sum them or call their differences leaked memory.

Concurrent backend cancellation edits are preserved. Actual measured identities match the adopted512 certificate on this execution path; the presence of a dirty file alone does not establish a runtime identity change. Original model/quality/compatibility preflight and exact CPU/backend count checks remain on the experimental route. This baseline does not refresh startup approval.

Testing original backend CLI prefill step2048 versus512 in symmetric2048/512/512/2048 order, without changing the KV/allocator budgets, model, sampling or token admission. Complete runs must retain identical source/identity/fixture hashes, exact counts/answers and background-load guards. Memory and latency comparison is pending; no speedup or reduced retention is claimed yet. All experimental failures are retained. Ordinary settings and backend/installed sources remain untouched by this investigation.

日本語: 使用中メモリと保持KVを分けた基準測定を完了し、prefill分割の往復比較を開始しました。並行変更は保持し、実行経路のidentityは採用時と一致することを確認しました。候補の効果と採用は未確定です。

简体中文：已完成区分活动内存与保留KV的基线测量，并开始prefill分块的对称比较。保留并行修改，已确认实际执行路径的identity与采用时一致。候选效果及采用仍待确认。
