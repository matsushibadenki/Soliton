# MLX SSD prefix cache — 2026-11-04

Soliton now offers an opt-in **MLX SSD** engine inspired by Kapitsa's persistent prefix cache. Rust owns its settings, qualification admission, private worker and lifecycle. Existing Strata, vLLM-Apple and Omni qualifications remain separate. The sections below preserve the earlier store/HTTP experiments; the managed-engine section describes the current integration.

## Backend investigation

The installed MLX-LM 0.32.0 provides `make_prompt_cache`, `save_prompt_cache` and `load_prompt_cache` in `mlx_lm/models/cache.py`. Its Safetensors payload includes per-layer state, metadata and cache class names. Restoring this state requires the compatible backend to reconstruct its classes and receive the exact matching token continuation. A matching text substring is insufficient: serialized chat templates, tokenizer behavior and token IDs matter.

The local `vLLM-apple/vllm_apple/mlx_server.py` exposes in-memory prompt-cache metrics and constructs the server response generator, but no reviewed persistent-cache import/export route was identified. Standalone MLX-LM serialization functions therefore do not establish that the running HTTP backend can safely import the same state. Strata, vLLM-Metal and Omni formats are not supported by this prototype. No claim is made that their upstream implementations lack state serialization; corresponding Soliton adapters still need review.

## Rust store

`crates/studio-core/src/prefix_store.rs` stores opaque backend state using these caller-supplied identities:

- model content SHA-256;
- tokenizer/template content SHA-256;
- runtime content SHA-256;
- exact token-prefix SHA-256;
- explicit backend state-format version.

A hashed key prevents file-name traversal. A changed identity returns a miss. Reads validate the header version, identity, bounded lengths, exact file size and SHA-256 before exposing a positioned file handle. State moves in 64 KiB chunks, not WebView messages or full-frame buffers. The store does not decode or certify backend tensor contents; the backend remains responsible for that.

The Unix prototype requires an owner-only, real directory and creates files with mode 0600. It rejects symlinks and serializes access with a file lock. Writes use a unique temporary file, sync it, atomically rename it, then sync the directory. Failed writes remove their temporary file and preserve the previously committed entry. A process crash may leave a temporary file; it counts toward quota. Explicit maintenance can remove recognized orphan files older than 24 hours.

Limits: 1 GiB per payload, 256 entries and a caller-selected total quota of at most 64 GiB. Header and temporary-write overhead count toward quota, including replacements. Full stores reject writes. Explicit stopped-engine capacity inspection and trimming are available below; automatic eviction remains disabled. The caller must handle miss or invalid-cache errors by recomputing the prefix before any generation. The managed MLX SSD adapter falls back to fresh prefill on invalid states.

## Reproduce the real-model check

Build the local CLI:

```sh
cargo build -p studio-core --example prefix-store
```

Use an existing Python environment containing MLX and MLX-LM, with an existing absolute local model path:

```sh
/path/to/mlx/python scripts/prefix-cache-roundtrip.py --model /absolute/path/to/local-mlx-model
```

The script uses offline loading with remote code disabled, a fixed synthetic prompt and a temporary directory. It computes content identities, materializes the prefix cache, saves it through MLX-LM, runs the Rust CLI to store and restore the bytes, verifies byte hashes, reloads the restored state with MLX-LM and compares greedy generation token IDs against a fresh-cache run. Temporary artifacts are removed on exit. No model is downloaded and no production setting is changed.

The CLI can separately store or retrieve an opaque payload:

```sh
cargo run -p studio-core --example prefix-store -- /private/cache-dir 67108864 put /path/to/identity.json /path/to/state.safetensors
cargo run -p studio-core --example prefix-store -- /private/cache-dir 67108864 get /path/to/identity.json /path/to/new-output.safetensors
```

The parent directory must exist. Retrieval creates a new output file and refuses to overwrite an existing one. Identity JSON uses `model`, `tokenizer`, `runtime`, `prefix` and `format` fields; the first four must be 64-character SHA-256 hex strings. A caller must compute them from the actual loaded artifacts, never invent placeholder hashes for real reuse.

## Evidence and remaining work

- 🟢 [Done] Rust store tests cover round-trip, identity changes, corruption, truncated input, quota rejection, preserving an older entry after failure and symlink refusal.
- 🟢 [Done] Real Apple Silicon / MLX 0.32.2 / MLX-LM 0.32.0 / Gemma 2 2B 4bit: 177 prefix tokens, 18,854,660-byte Safetensors payload, byte-identical Rust round-trip and 11/11 greedy output token IDs matching the fresh-cache run. Source model SHA-256 remained `3e8d02db8dc0df26d4e45291ae3a11ba9538671fc5337b22a8176f611a24b800`.
- 🟢 [Done] Managed MLX SSD lifecycle, exact-token admission, private transport, renewed qualification and invalid-cache fallback; see below.
- 🟢 [Done] Write deduplication and explicit stopped-engine capacity management, including recognized old temporary files.
- 🟠 [Next] Automatic eviction with its own runtime qualification.
- 🔴 [Later] Foreground-priority history preparation and measured SSD-cache latency/memory tradeoffs on representative conversations.
- ⭕️ [Pending] Other backend formats, model families and Macs require corresponding implementations and measurements.

This initial round-trip check alone does not establish inference speedup, whole-model quality or concurrent inference correctness.

## Opt-in HTTP integration — 2026-11-04

`scripts/prefix-cache-http.py` connects the Rust store to a separate authenticated MLX-LM HTTP process. It is now launched by the separate MLX SSD engine after its own evidence checks; it does not replace another engine or reuse its qualification. The experiment supports the reviewed Gemma 2 2B layout and MLX-LM 0.32.0 source only; server, generation and cache modules have exact source guards. Runtime identity also includes the wrapper/helper source, Rust store binary, MLX binary/version, OS version, architecture and SoC. Changing any of these produces a new disk-cache identity.

The supported route is `POST /v1/chat/completions`, bound to `127.0.0.1` only. Set a private printable-ASCII `SOLITON_PREFIX_HTTP_TOKEN` of at least 24 characters in the process environment and send `Authorization: Bearer <token>`. The key is never written to the cache. `GET /studio/ssd-cache` uses the same authentication and returns scalar hit/miss/write/error counters, with `experimental:true` and `productionIntegration:false`.

Supported requests use `default_model`, text-only system/user/assistant messages, greedy temperature zero, seed zero, at least two input tokens and 1–128 output tokens, with input plus reserved output at most 512. One HTTP generation is admitted at a time; competing generation requests return 503. JSON and SSE are supported. Other request fields, models, adapters, speculative decoding, batching and media inputs are disabled. This deliberately narrow service is for correctness experiments, not general-purpose serving or untrusted-network deployment.

On a memory-cache miss, the backend asks the Rust store for the exact input prefix excluding its last token. Verified states are decoded by MLX-LM and checked for the expected layer count, token-prefix metadata and per-layer offsets. A miss, corruption or restore failure returns a fresh-cache request to the original generation path. The existing prefill progress callback captures state immediately before processing the last input token, so there is no second prefill pass just to create a disk cache. Loaded states are not re-saved on the same request. Restored prefixes participate in upstream `cached_tokens` accounting; cold prefill is not mislabeled as a cache hit.

A failed write or insufficient staging capacity does not fail generation. State serialization and Rust CLI access are synchronous in this prototype, with a 20-second CLI timeout; they can add first-output latency and temporary disk usage. Store quota covers committed entries and Rust atomic-write overhead, not the worker's temporary Safetensors staging file. The worker separately checks free space and a conservative tensor-size allowance before staging. A small configured quota can skip staging even when a more tightly serialized state might fit. Restore verification failures currently count as misses; the `errors` counter is not a detailed corruption audit.

After building the Rust example, use a compatible MLX Python:

```sh
/path/to/mlx/python scripts/prefix-cache-http.py \
  --model /absolute/path/to/gemma-2-2b-it-4bit \
  --cache-root /private/tmp/soliton-prefix-http-cache \
  --quota-mib 256 --port 8082 --evidence /private/path/to/qualification.json
```

The cache root's parent must exist. The root must be a real, owner-only directory. Cache entries contain model states derived from prompts; keep the directory private. Soliton launches this worker only when MLX SSD is explicitly selected and its configured qualification evidence passes. `--qualification-mode` is reserved for isolated certification, never normal application startup.

Reproduce the isolated HTTP regression (the token is generated in memory, the model is local, the store is temporary, and owned server processes are stopped on completion):

```sh
/path/to/mlx/python scripts/prefix-cache-http-smoke.py \
  --python /path/to/mlx/python \
  --model /absolute/path/to/gemma-2-2b-it-4bit
```

Measured on the existing Apple Silicon / MLX 0.32.2 / Gemma 2 2B 4bit environment:

- 🟢 [Done] Cold HTTP request: disk misses 1, writes 1, no disk hit.
- 🟢 [Done] After a full server-process restart: disk hits 1; upstream reports 177 cached tokens out of 178 input tokens; the 11-token answer matches the cold response text.
- 🟢 [Done] A deliberately corrupted payload produces a miss and fresh prefill, followed by the same correct answer and a replacement cache write.
- 🟢 [Done] An SSE client disconnect is followed by a successful ordinary request.
- 🟢 [Done] A full logical quota refuses a new cache write while the HTTP request still returns the correct answer.
- 🟢 [Done] Managed engine launch/stop, token admission, three-language settings and fresh 30-minute qualification are implemented below.
- 🟠 [Next] Broader workload coverage, cancellation timing and controlled latency/memory comparisons.

These HTTP results establish this narrow experiment's behavior. They do not establish a latency/memory improvement. The managed host is tested separately below.


## Managed Soliton engine and requalification

Preferences → Engine → **MLX SSD** exposes model, MLX Python, runtime scripts directory, Rust `prefix-store` executable, private cache directory, qualification file and quota (64–4096 MiB). Set absolute paths while stopped. The fixed profile is Gemma 2 2B 4bit, text chat, total context 512, maximum output 128, temperature 0, seed 0 and concurrency 1. No model download or dependency install is automatic. Runtime files and Python are external prerequisites, not a self-contained distribution.

Build `prefix-store` first, then certify the exact artifacts and private cache destination:

```sh
/path/to/mlx/python scripts/qualify-prefix-cache.py \
  --python /path/to/mlx/python \
  --model /absolute/path/to/gemma-2-2b-it-4bit \
  --store-executable /absolute/path/to/prefix-store \
  --cache-root /private/existing-parent/mlx-ssd-cache \
  --quota-mib 256 --output /private/path/to/new-qualification.json
```

The output must not already exist. The full run takes at least 30 minutes; `--preflight` checks prerequisites but cannot issue usable evidence. Keep the evidence owner-only. Rebuilding the store or changing runtime code/dependencies, model/tokenizer, cache location, quota or machine requires new qualification. Evidence expires after seven days. A checked-in measurement summary is not a replacement for local private evidence.

Rust verifies evidence before spawning, and the worker verifies it again before loading weights. The content identity includes model/template, Python and dependency inventory, worker sources, store executable, OS/SoC/RAM and cache destination/device/quota. Launch and inspection remove inherited Python overrides and numerical MLX/Metal/OpenMP/tokenizer flags so a native app reproduces the fixed certified environment. No global environment is changed. Worker authorization uses an ephemeral environment token and a private loopback port; the token is absent from command arguments and persisted settings.

The existing Rust lifecycle owns the process group, refuses duplicate starts, locks engine configuration while live, and stops the owned process on stop or engine switch. Both API and GUI requests undergo strict shape checks and an authenticated exact token count before inference; the GUI checks before appending history. A failed/malformed counting contract latches admission closed until restart. No large cache/tensor data crosses WebView IPC. Settings and metrics support English, Japanese and Simplified Chinese; metrics clear when stopped.

### Measured qualification

- 🟢 [Done] Full local run: **1800.302 seconds, 2953 successful requests, zero failures**, sampled RSS growth **4,669,440 bytes (4.45 MiB)**. Before/after artifact identity matched.
- 🟢 [Done] Authentication and invalid-field rejection, cold execution, restart disk reuse, corruption recompute, SSE disconnect recovery and quota-full fallback passed. Exact context boundary: 496 input + 16 output accepted; reserving 17 output rejected.
- 🟢 [Done] Native Soliton host accepted the evidence, reached ready, answered `4` to `2 + 2`, and displayed live SSD metrics that cleared after stopping. Managed-host regression covers private metrics, 177/178 cached input tokens, JSON/SSE answer parity, settings lock, stop/restart, engine switching, conversation preservation and wrong-quota/expired-evidence rejection.
- 🟢 [Done] 136 Rust tests, Python evidence-contract tests, frontend build and three-language 1280/390/320 px UI checks.
- 🟢 [Done] The original soak recorded 2948 writes, motivating the write deduplication and measured comparison below. These historical qualification figures precede that optimization.
- 🔴 [Later] Broader contexts, batching, model families and automatic cache eviction.
- ⭕️ [Pending] Other Macs, backend formats and signed/notarized distribution verification.

The worker reports `experimental:true` / `productionIntegration:false` during isolated qualification and the opposite during evidence-checked managed startup. Raw soak measurements retain qualification-mode metadata. The engine remains opt-in. These measurements are functional/stability evidence on one development Mac, not general model-quality or performance certification.

The explicit isolated-host regression command must only target a disposable QA application state:

```sh
node scripts/prefix-engine-managed-smoke.mjs --isolated \
  --api http://127.0.0.1:8080 --output /tmp/mlx-ssd-managed.json
```

[Sanitized qualification and native-host validation record](evaluation/mlx-ssd-managed-20261104.json). Private startup evidence stays in the local runtime directory.

## Repeated-write suppression and performance comparison

Successful Rust `put`/`get` operations now return a bounded JSON receipt identifying the opaque store entry. The worker remembers up to 256 committed prefixes. Before serializing a repeated prefix, it checks that the entry is still a private regular file and that device, inode, size, mtime, ctime, owner and mode match the receipt. Store-directory replacement or permission changes also invalidate the receipt. Unchanged entries skip Safetensors serialization and Rust `put`; different prefixes still save normally. A failed write is never remembered, so freeing quota permits retry. The memo is process-local and bounded; it does not suppress the normal checksum and backend-shape validation when restoring state.

This optimization avoids redundant persistence; a request served from RAM still uses RAM rather than being counted as an SSD read. New scalar metrics are `skippedWrites`, `serializedBytes` and `committedBytes`. The three-language UI shows skipped writes, clears metrics on stop and discards refresh responses that finish after stopping. Byte counters count serialized payloads/logical commits, not physical SSD/NAND writes or total disk I/O (temporary files, headers and filesystem overhead are separate).

The real-model comparison used serial **ABBA** order: baseline → optimized → optimized → baseline, each with a fresh private cache, the same Python/model, greedy seed 0 and output limit 16. Each run includes one cold request, 20 repeats, 20 alternating codeword/arithmetic requests and one request after a full worker restart. All 168 answers matched their corresponding fixture, with 177/178 input tokens reused after restart. Latency starts immediately before the client HTTP request and ends at first nonempty SSE content or stream completion. Model startup and Rust gateway overhead are excluded.

| Scenario | Requests per version | Writes before → after | First-content median before → after | Completion median before → after |
| --- | ---: | ---: | ---: | ---: |
| Same prefix repeated | 40 | 40 → 0 | 369.8 → 161.8 ms | 554.8 → 344.9 ms |
| Two alternating prompts | 40 | 40 → 2 | 358.9 → 265.1 ms | 487.7 → 346.1 ms |
| Cold | 2 | 2 → 2 | 828.6 → 704.9 ms | 1012.6 → 889.8 ms |
| After worker restart | 2 | 0 → 0 | 421.9 → 392.3 ms | 605.4 → 575.8 ms |

For repeats, first-content median decreased 56.2% and completion median 37.8%; p95 first-content was 456.2→180.4ms and completion 671.5→364.4ms. Repeated commits avoided 754,186,120 payload bytes (719.25MiB) across 40 requests. Both optimized runs separately had repeat medians near 160–164ms, versus about 370ms for both baseline runs. Alternating workloads saved 95% of commits. RSS sample maxima were approximately 2.10GiB in both versions; this does not establish a memory reduction.

Cold/restart rows have only two observations each and should not be interpreted as speedup evidence. OS page cache, background desktop activity and thermal conditions were not controlled. These results apply to this narrow local synthetic workload, not arbitrary conversations, physical write endurance, network service latency or another Mac. [Full samples, hashes, per-run identities and summary](evaluation/mlx-ssd-write-comparison-20261104.json).

```sh
# Preserve the baseline runtime directory and store executable before applying changes.
/path/to/mlx/python scripts/benchmark-prefix-writes.py \
  --python /path/to/mlx/python --model /absolute/path/to/model \
  --baseline-runtime /absolute/path/to/saved-baseline-scripts \
  --baseline-store /absolute/path/to/saved-baseline-prefix-store \
  --runtime /absolute/path/to/current/scripts \
  --store /absolute/path/to/current/prefix-store \
  --output-dir /private/existing-parent/new-comparison --repeats 20
```

The output directory must be new. The benchmark uses isolated qualification mode, owns and stops its worker processes, and never issues startup evidence. All cache identities change when the worker/helper or store binary changes; prior evidence must be replaced by a full fresh qualification. The extended qualifier also checks write suppression, live entry repair and quota-recovery retry before its 30-minute soak.

- 🟢 [Done] Bounded receipts, unchanged-entry suppression, live corruption/deletion repair, failed-write retry, scalar metrics and ABBA comparison.
- 🟠 [Next] Broader multi-turn workloads, controlled disk/page-cache experiments and explicit eviction/orphan management.
- ⭕️ [Pending] Other Mac/model performance and physical SSD write measurement.


The optimized runtime completed a fresh **1800.730-second qualification: 2458 successes, zero failures, sampled RSS growth 9,142,272 bytes (8.72MiB)**, with matching before/after artifact identities. All original gates plus duplicate-write suppression, live entry repair and quota-recovery retry passed. The final worker session recorded 3 saves, 2441 skipped saves and no cache errors; counters reset on each process restart, so these are not cumulative counts for every qualification phase. This is stability evidence, not a throughput comparison with the earlier soak. Local startup evidence remains private; the public JSON contains a measurement copy only.

The isolated native Soliton host then accepted the new private evidence and passed managed startup, disk reuse, JSON/SSE parity, duplicate-write suppression, exclusive start/settings lock, admission refusal, owned process cleanup, restart reuse, wrong-quota/expired-evidence refusal, engine switching and conversation preservation. Existing ordinary application settings were not changed. Rust 136 tests, receipt 3 tests, evidence-contract 4 tests, frontend/native builds and EN/JA/ZH UI fixtures passed.

## Multi-turn workloads and capacity management

The MLX SSD preferences now include an on-demand **SSD capacity** panel and a stopped-engine trim action. Rust owns both operations. The report separates recognized saved states (including headers), UUID-v4 temporary files, protected/unrecognized regular files, entry counts, quota and filesystem free space. The saved-store figures cover only `store`; the separate staging snapshot added on 2026-11-05 is described below. Reads use bounded metadata inspection, not full payload hashing; normal state restoration still verifies checksums.

Trimming removes recognized temporary files older than 24 hours, then the oldest saved states until usage is at most **75% of the configured quota**. Ordering uses file modification time, not last cache access. It protects recent temporary files, non-v4 names and unknown files. If protected files prevent the target, the result explicitly says the target was not reached. Symlinks/unexpected directories abort inspection before deletion. Scans are bounded to 4096 directory entries. Store locks are nonblocking for maintenance; a busy store returns 409 for retry. Locks explicitly unlock on drop so a concurrent fork cannot retain a completed operation's lock.

`GET /studio/prefix-capacity` and authenticated `trimPrefixCache` use saved settings and the Rust lifecycle lock. Trimming is refused while there is an owned engine process/group, live native worker, active API request or GUI generation. No raw state bytes cross WebView IPC. A missing store reports empty without creating the cache directory. GUI labels/tooltips are available in EN/JA/简体中文, with compact units suitable for 320px layouts.

The previously qualified inference worker/helper files and `prefix-store` executable remain byte-identical. The new maintenance logic runs in the Rust control plane; the separate `prefix-maintain` example supports isolated test stores without replacing the qualified store executable:

```sh
cargo build -p studio-core --example prefix-maintain
# Inspect an existing private store:
/absolute/path/to/prefix-maintain /private/cache/store 268435456 stats
# Explicitly trim that store while its engine is stopped:
/absolute/path/to/prefix-maintain /private/cache/store 268435456 trim
```

Rebuilding `prefix-store` itself still requires checking/requalifying the resulting inference artifact identity. Do not substitute `prefix-maintain` for it. The maintenance CLI operates on its explicitly selected store and uses the same nonblocking lock; the Soliton API additionally enforces the stopped-engine rule.

The multi-turn test uses three synthetic practical scenarios: English workshop room/time changes, Japanese shopping quantities and Chinese destination/departure-time changes. Each conversation grows through six turns; assistant replies become part of later requests. Every request is repeated, then the store is trimmed under a 128MiB pressure quota. Retained states and evicted states are probed in separate restarted processes to distinguish disk reuse from any short common RAM prefix. Every canonical history is also run in a fresh process/store and compared for exact response-text parity. An oversized continuation must be rejected rather than silently dropping history. The existing 512-token total context and 32-token output reservation are retained.

```sh
/path/to/mlx/python scripts/validate-prefix-multiturn.py \
  --python /path/to/mlx/python --model /absolute/path/to/gemma-2-2b-it-4bit \
  --store /absolute/path/to/qualified-prefix-store \
  --maintain /absolute/path/to/prefix-maintain \
  --output-dir /private/existing-parent/new-multiturn-run
```

The output directory must be new. The script selects free loopback ports, uses isolated qualification mode, owns/cleans its worker processes, and does not issue qualification evidence. Fixture answers and timing samples are functional observations, not a broad model-quality or controlled performance benchmark. It does not read real user conversations.


### Measured multi-turn and capacity results — 2026-11-04

- 🟢 [Done] Three six-turn conversations, 18 canonical histories and 18 repeats completed. All 18 histories matched the exact response text from separate fresh processes/stores. Inputs ranged 23–155 tokens, with 32 output tokens reserved, within the fixed 512-token profile. Oversized continuation rejection passed.
- 🟢 [Done] Semantic fact checks 9/9: updated room/time, milk/bread quantities and destination/time were retained. This is a small synthetic factual-recall fixture, not broad reasoning or long-context qualification.
- 🟠 [Next] Strict output-format adherence was 9/18 after whitespace normalization. English/Chinese acknowledgements added a period (`OK.`); Japanese count replies included units (`1本`, `3個`), and the final shopping line retained units. These same outputs were reproduced from fresh state. Cache correctness passing does not mean model instruction adherence passed.
- 🟢 [Done] Under 128MiB quota, the growing/repeat phases committed 15 states and refused 6 write attempts at capacity, while all 36 requests still returned matching answers. These expected write refusals appear in cache-error counters; they are not inference failures. Failed puts were not memoized.
- 🟢 [Done] Explicit trim removed 5 old states / 40,725,866 bytes (38.84MiB): used 128,141,406→87,415,540 bytes (122.21→83.37MiB), leaving 10 states below the 96MiB target. Retained-prefix disk restore, evicted-prefix recompute/save and the subsequent duplicate-write skip passed, each with matching text.
- 🟢 [Done] Rust 140 tests and frontend/native builds passed. Real Rust API UI tests in EN/JA/简体中文 at 1280/390/320px verified capacity refresh, trim, protected files, history preservation, running-state disablement and no app console/overlay errors. Browser skill was not listed, so installed Playwright/Chrome was used. Native CUA also verified real usage 89MiB / 4 entries under 256MiB quota.
- 🟢 [Done] The updated native host accepted the existing matching private qualification evidence and passed capacity inspection, live trim refusal, stopped maintenance, inference/reuse, restart, exclusive settings, expired/wrong-quota rejection, switch cleanup and conversation preservation. Worker/store hashes remained unchanged.
- 🟢 [Done] Separate stopped-engine staging cleanup with bounded identity checks and protected file handling; see below.
- 🟢 [Done] 2026-11-06 automatic-eviction selected profile qualified; see the completed qualification below.
- 🟠 [Next] Near-limit longer multi-turn workloads and stricter model output-format evaluation. Manual trim creates space but does not guarantee a particular next serialized state fits.
- ⭕️ [Pending] Other Mac/model validation and physical SSD-write measurements.

[Canonical histories, raw replies, format checks, counters, trim and native-host evidence](evaluation/mlx-ssd-multiturn-capacity-20261104.json). Timing samples were collected during development/desktop activity in separate phases, without ABBA or controlled thermal/page-cache conditions; they must not be treated as a cache speedup comparison. The earlier ABBA measurement remains a separate experiment. The fixture uses synthetic content and has no real user conversation data.

The first test attempt incorrectly expected a disk miss after priming a retained RAM prefix; separate restarts fixed that distinction. Another attempt encountered a port used by an unrelated application and stopped its own worker; the successful run selected free ports. No unrelated process was terminated. Earlier incomplete attempts did not produce passing evidence.


## Staging accounting — 2026-11-05

The capacity endpoint now returns a separate `staging` snapshot. Python's `stage-xxxxxxxx` folders live beside `store`, so their files were not represented in the saved-store quota. Rust now reports logical file bytes, folder counts, stale-candidate bytes/counts, and other direct-file bytes/counts. The UI displays staging separately from the quota bar in English, Japanese and Simplified Chinese. A refresh error clears the previous capacity snapshot.

A stale candidate requires a private owner-owned directory with the reviewed eight-character temporary-name suffix, at least 24 hours since the directory and every child file were modified, and only regular, current-user, single-link `identity.json` / `state.safetensors` children. Unknown child filenames remain included in staging bytes but exclude the whole folder from stale candidates. Empty old staging folders can be candidates with zero bytes. Age/name recognition alone does not authorize deletion. The initial accounting phase does not remove staging folders; the separate stopped-engine cleanup action described below uses stricter eligibility. Saved-store trim still does not remove staging folders.

The scan is read-only, one level deep and bounded to 4,096 total directory entries. Symlinks, unexpected directories, inaccessible or disappearing entries fail the snapshot instead of producing a partial total. The saved store is inventoried separately by its existing private lock and header rules. Missing `store` does not hide existing staging. A running-worker snapshot is not atomic and can fail if a temporary folder disappears during scanning; refresh retries without model loading or inference changes. Sizes are logical lengths, not physical blocks, NAND writes or a combined quota guarantee. Unknown directory trees are never traversed.

Verification: all 147 Rust core tests passed, including mixed recent/stale/unknown fixtures, links/nesting/limit/private-directory refusal, and stopped-engine trim preserving staging and conversation state. An isolated real API and Playwright/Chrome suite measured staging `4,195,330` bytes with `4,194,306` stale-candidate bytes across two folders; it verified missing-store accounting, authentication, failure clearing/recovery and EN/JA/ZH at 1280/390/320px. File contents are synthetic; no actual crash-recovery deletion, live eviction, native-window verification or performance gain is claimed. Browser plugin was unavailable because the browser skill was not listed; installed Playwright was used. Frontend production build passed with the existing Lucide directive warning. Qualified worker scripts and prefix-store executable retained their prior SHA-256 values.

Reproduce against a disposable offline server (not ordinary app data):

```sh
STUDIO_STATE_PATH=/tmp/soliton-staging-qa/state.json STUDIO_PORT=8081 cargo run -p studio-server
npm run dev
node scripts/prefix-staging-ui.mjs --isolated --api http://127.0.0.1:8081 --url http://127.0.0.1:5180
```

The UI script restores the initial settings and deletes only its own synthetic temporary folder. It never starts an inference worker. [Machine-readable checks](evaluation/ssd-staging-accounting-20261105.json).


## Stopped staging cleanup — 2026-11-05

A separate **Clean orphan staging** icon in the staging heading calls authenticated `cleanupPrefixStaging`. Rust holds the engine lifecycle lock and refuses the action when a managed process/group/worker, active API request or GUI generation remains. The existing saved-store trim is a separate action; cleanup never trims saved prefixes or store temporary files. Nothing is automatically deleted on refresh or startup.

`staging.cleanableBytes` and `cleanableDirectories` distinguish deletable folders from the broader old-candidate inventory. In addition to every prior ownership/name/age/regular-file/single-link requirement, a folder must contain a readable `identity.json` of at most 4,096 bytes, parse as the strict store identity schema with content SHA-256 fields and the reviewed `mlx-lm/0.32.0/gemma2-2b/http-prefix-v1` format. Missing/invalid manifests, unknown files, recent children, hardlinks and unknown formats remain protected. A manifest-only old folder can be cleaned after an interrupted restore. No model or state payload is loaded to make this decision; a syntactically valid manifest is artifact recognition, not independent proof of origin or current-model qualification. Only use this action on the dedicated selected cache root.

The whole staging and saved-store inventories are preflighted before deletion. Cleanup pins a real no-follow root directory descriptor and takes a nonblocking exclusive flock on that directory; another cleanup receives HTTP409. Each candidate and every child is rechecked against the captured device/inode/owner/mode/link count/size and nanosecond modification/change timestamps. Files are unlinked relative to pinned no-follow directory descriptors; identity is removed last. Empty-folder removal is nonrecursive. Changed folders are skipped/reported; new unknown children cannot be recursively removed. This is not a transactional filesystem operation: partial failures can reclaim some files, and a post-operation rescan failure returns an error requiring refresh. All workers using this root must be stopped, including independently launched experiments: the existing standalone inference worker does not participate in the new maintenance flock. Concurrent noncooperating external writers are outside this maintenance contract.

The response includes refreshed store/staging/free-space data plus `stagingCleanup` before/after snapshots and removed-byte/file/directory and failed-directory counts. The UI reports reclaimed bytes, disables cleanup when no eligible folder remains, and warns if candidates changed or failed removal. EN/JA/ZH preserve compact icon controls and information tooltips. Logical removed bytes are not a physical disk-reclamation or NAND-write measurement.

Verification: 151 Rust tests passed, including recent/invalid/unknown preservation, bounded identity parsing, wrong formats/hashes, manifest-only interrupted restore, hardlinks, changed files, swapped folders, full-scan refusal before deletion, busy root lock, idempotence, active worker/API/GUI refusal, and existing history/settings preservation. The isolated real API → Playwright/Chrome suite exercised cleanup in EN/JA/ZH with 1280/390/320px views. Each language trial removed exactly two synthetic files / one directory / 2,097,510 logical bytes with zero failed folders; repeat cleanup removed zero. Invalid old manifests, recent files, other root files and a protected saved-store file remained. Unauthenticated cleanup returned401; link refusal cleared stale UI and recovered after the fixture link was removed. No unexpected page/console errors or horizontal overflow. Frontend and native Tauri builds passed (existing Lucide bundler warning only). The previously qualified worker scripts and prefix-store executable hashes are unchanged. No real inference-worker crash, live eviction, new performance result or native-window interaction is claimed.

Reproduce with the disposable host and `scripts/prefix-staging-ui.mjs --isolated` command above. The suite now tests cleanup as well as accounting and restores settings/removes only its own fixtures. [Checks](evaluation/ssd-staging-cleanup-20261105.json).


## Pressure-eviction candidate — 2026-11-06

The separate `prefix-store-evict` executable uses `Store::put_evicting`. Ordinary `put` and the existing managed worker remain refusal-only; no settings/evidence were changed. The candidate accepts the same `put|get` CLI and adds an `eviction` receipt with removed entry/file bytes on successful put.

It triggers only when incoming payload plus the fixed 4,104-byte header region or the 256-entry limit would overflow. Under the existing store lock, a bounded inventory plans oldest-save-first reclamation toward 75% **after** the incoming reservation, when possible. This is not LRU. The destination, unknown files, recent temporary files and hardlinked candidates stay protected. A complete feasibility check precedes deletion; impossible reservations do not delete entries. Candidate metadata is checked before and during deletion. Only recognized old states or eligible old store temporary files can be removed; worker staging is outside this policy. A fallback can fit under quota without attaining the preferred headroom if protected files prevent it.

Writes keep the prior atomic temporary-file/checksum/rename flow and preserve an overwrite target if the new stream fails. Eviction is not transactional: a later source, write, filesystem or sync failure can leave other recomputable prefixes removed, even though no new receipt is returned. Quota includes the full incoming temporary file while an overwrite target still exists. Logical quota does not prove physical block reclamation: an already-open get handle remains readable after unlink. Receipt invalidation already checks on-disk existence; evicted prefixes are eligible for recomputation and re-save.

Build/test without replacing the qualified executable:

```sh
cargo build -p studio-core --example prefix-store-evict
cargo test -p studio-core --lib
python3 scripts/prefix-eviction-store.test.py
```

All156 Rust tests pass, including quota/header reservation, entry-limit pressure, recent-temp and unknown/hardlink preservation, symlink refusal, impossible reservation without deletion, failed overwrite preserving its target, reinsert and a pinned reader after unlink. CPU-only CLI comparison used ten20KiB opaque payloads and a100,000-byte quota: original saved4/refused6; candidate saved10/refused0 with6 evictions. Reinsert/get SHA-256 and receipt invalidation passed. These files are synthetic opaque bytes, not serialized MLX state; this is not an inference/latency result. Frontend/Tauri builds pass; no UI change in this phase.

`validate-prefix-eviction.py` provides a separate isolated qualification-mode real-model pressure/restart/recompute comparison, but its initial runs **did not pass**; the selected-source review and completed qualification below resolve that blocker for the isolated profile. The installed environment is now MLX-LM0.32.3.dev12+g4cfc64784 / MLX0.32.4 and fails the reviewed-version guard. A task-owned temporary environment with PyPI MLX-LM0.32.0 still failed the three-module source-hash guard. The guard was retained. Review the actual source differences and qualify the complete environment before using this candidate in managed inference. Existing qualified executable/worker file hashes are unchanged, but this does not qualify the newly installed dependencies. No30-minute candidate evidence or managed adoption is reported. [Checks and guarded-source diagnostics](evaluation/ssd-eviction-candidate-20261106.json).


## Selected backend source review — 2026-11-06

The installed development backend (`mlx-lm0.32.3.dev12+g4cfc64784`) differs substantially from the available PyPI0.32.0 source: its server adds vision/image handling and changes response state/stop processing, while the reviewed cache-module bytes are identical between those two available installations. The old approved installation's original source bytes were unavailable; prior hashes are preserved in the review record, so this is a fresh review of selected source contracts rather than a reconstructed diff against those unavailable bytes. The development backend is not approved by this work.

The selected environment is isolated at `.soliton-runtime/ssd-eviction-20261106/python`, with PyPI MLX-LM0.32.0 package bytes and existing MLX0.32.4 dependencies. Exact module hashes remain mandatory. Review covered fixed model/adapters and sequential response generation, the prefill callback before the final prompt token, evaluated state/26-layer offsets, Safetensors classes/scalars/metadata, deep-copy/trim behavior, memory-cache bounds and disconnect/thread cleanup. The wrapper now explicitly supplies unquantized `kv_bits=None`, `kv_group_size=64`, and `quantized_kv_start=5000`, which this selected server requires. No context, output or concurrency limits were raised. Existing certificates bind different worker/runtime content and cannot authorize this environment. [Source review](evaluation/ssd-backend-source-review-20261106.json).

The real128MiB pressure comparison passed against the selected environment. Ten distinct prompts produced matching cold replies: refusal-only saved4 with6 storage errors, while automatic eviction saved10 with0 storage errors and removed6 older states. Unknown files survived and logical store usage stayed within quota. A retained state restored after restart; another restart cleared RAM before requiring a genuine disk miss for an evicted prefix, followed by recomputation/save and deduplication. Source-model content SHA-256 remained unchanged. These request/state counts do not establish a latency, NAND-write, memory or broad model-quality improvement. [Raw real-model comparison](evaluation/ssd-eviction-real-20261106.json).

The qualification controller now supports `--require-eviction` for64–256MiB stores. It retains every original authentication/admission/corruption/disconnect/quota/context gate and adds22 distinct pressure prompts in the certificate's own store, unknown-file protection, retained-state restart restoration and cold evicted-prefix recomputation. An impossible reservation must leave the previous saved filenames intact. Evidence declaring `requires_eviction=true` must contain both new eviction gates; contract tests reject missing gates and non-boolean declarations. The30-minute soak begins only after these gates and warmup, with the original128MiB RSS-growth bound and before/after full runtime identity checks. Managed settings/ordinary history are not automatically changed.


### Qualification handoff correction

The first full attempt passed every initial gate but stopped on HTTP503 after the last successful progress sample at1,123.3s /2,079 total requests. No evidence was issued. Its response body was not retained, so the exact503 source could not be proven retrospectively; the worker log contained no generation traceback. Review identified a reproducible admission race: a serial client can consume the complete Content-Length response before the previous serving thread reaches its admission-lock `finally` release.

The wrapper now waits at most100ms for the prior handler's teardown, while retaining one inference slot and refusing a longer-held slot. This is server-side bounded admission, not client retry or concurrent model generation. Four CPU checks cover the short handoff, long-held refusal, multiple-waiter exclusion and bounded HTTP-error diagnostics. The HTTP error helper preserves status codes so expected401/422 gates remain strict. An initial diagnostic-helper bug stopped the second attempt during authentication; it was fixed/tested before the next full run. Both failed attempts remain recorded and contribute no qualified time. [Attempt1](evaluation/ssd-eviction-qualification-attempt1-20261106.json), [attempt2](evaluation/ssd-eviction-qualification-attempt2-20261106.json).


### Completed automatic-eviction qualification

The corrected selected environment passed the full30-minute qualification: **1800.427s /3218 total successful requests /zero failures /0 sampled RSS-growth bytes**. All14 gates passed, including exact496+16 acceptance /one-token-over refusal, corrupt-state fallback, duplicate suppression/live repair, disconnect recovery, protected quota refusal/retry, automatic eviction and restart/cold recomputation. Model/runtime content identity matched before and after the soak, and the worker shut down cleanly. Total successes include the initial gates and warmup, not only soak requests. RSS is sampled worker growth after warmup, not whole-process/GPU peak memory or a memory-reduction claim.

In the certificate's own256MiB store,22 distinct pressure requests all saved successfully with0 new storage errors and23 older entries removed (including earlier gate fixtures). Every observed logical usage stayed within quota and unknown-file contents remained intact. A retained state restored after restart; a second restart cleared RAM before requiring a genuine SSD miss, recomputation/save and subsequent skipped duplicate write. This is separate from the earlier128MiB comparison against refusal-only behavior. [Raw certificate copy](evaluation/ssd-eviction-qualification-20261106.json).

The private, owner-only operational certificate is `.soliton-runtime/ssd-eviction-20261106/qualification-v3.json`, with cache root `.soliton-runtime/ssd-eviction-20261106/qualified-cache-v3`, Python `.soliton-runtime/ssd-eviction-20261106/python/bin/python`, scripts in `scripts`, and executable `target/debug/examples/prefix-store-evict`. Settings use256MiB, fixed Gemma2 2B4-bit, context512 including output, maximum output128, greedy seed0, memory cache64MiB /allocator256MiB and one inference at a time. Exact content, cache path/device, expiry and every evidence gate remain required. Moving the cache or changing dependencies/settings needs corresponding new qualification. Ordinary app settings/history are not migrated by this validation.

Reproduce with fresh private output/cache paths, using the selected Python and the real model:

```sh
/path/to/selected/python scripts/qualify-prefix-cache.py \
  --python /path/to/selected/python --model /absolute/gemma-2-2b-it-4bit \
  --cache-root /absolute/new-private-cache \
  --store-executable /absolute/Soliton/target/debug/examples/prefix-store-evict \
  --output /absolute/new-qualification.json --quota-mib 256 --port 18088 \
  --require-eviction
```

The first failed soak and initial diagnostic failure remain failure records, not approvals or qualified time. The successful certificate covers the corrected worker only. The currently installed development backend and other model/platform/context profiles remain unqualified by this work. No controlled performance speedup, physical NAND-write reduction or broad output-quality improvement is claimed.


Rust-managed isolated-host checks also passed with this certificate: actual startup, exact177-token disk reuse, SSE, duplicate-write suppression, restart reuse, exclusive start, settings locks, context/tool/output admission refusal, live maintenance refusal, wrong-quota/expired-certificate refusal, engine switch/recovery, owned process cleanup and unchanged conversation history. Stopped maintenance completed after the checks. The older certificate was separately refused against the new identity. No ordinary app settings were migrated and no native-window interaction was added in this backend-only pass. [Managed checks](evaluation/ssd-eviction-managed-20261106.json), [combined result](evaluation/ssd-eviction-requalification-20261106.json). Rust156, admission/diagnostic4, evidence-contract5 and write-receipt3 tests passed; the frontend/native builds from the candidate phase remain unchanged by these Python-only edits.


## Nine-turn near-limit conversations — 2026-11-06

`validate-prefix-multiturn.py` now has `--auto-eviction --near-limit --quota-mib 256` mode. English, Japanese and Simplified Chinese each retain nine actual user turns and the generated assistant replies. Only the terminal user question is padded with neutral notes; no earlier turn is removed. The selected local tokenizer constructs496 input tokens, the worker's tokenizer independently confirms the count, and streamed usage must agree. Reserving16 output tokens reaches512; reserving17 must reject with422 before inference. Each final request contains17 messages. A separate oversized continuation is also refused.

The run passed84 streamed requests:27 growing histories,27 exact-repeat probes,27 fresh-process/fresh-store references, retained-state restoration, cold evicted-state recomputation, and a duplicate-write probe after repair. All27 history replies matched the fresh reference. All3 exact boundaries passed and all3 one-token-over reservations were rejected. Growing requests saved27 states and automatically evicted22 older states; all phases reported0 storage errors. Observed saved-store usage peaked at267,503,026 logical bytes within the268,435,456-byte quota. This is a sampled committed-store figure, not a peak staging/physical-allocation measurement. Restart probes used the same run identity; RAM was cleared before the evicted-prefix miss/recompute check. No manual trim was used in automatic mode.

Quality remains separate from cache correctness. The18 explicit fact-marker checks passed, but these are literal fixture checks, not semantic certification. Exact single-line format passed15/27. Failures were extra periods after `OK` and Japanese counters (`1本`, `3個`) despite requests for numbers only. There were no empty or output-truncated canonical replies. Format assessment v2 permits whitespace around `|`, `/` and `:` separators but rejects line breaks, extra prose and spaces inside factual values. Four CPU regressions cover extended-turn mapping, extra prose/wrong values, previously hidden newline/internal-space errors, and invalid near-limit configurations failing before an output directory is created. Raw model replies are never rewritten.

The real run retained its original raw report privately. The public evidence re-scores those same raw replies with the stricter v2 format assessor and renames legacy `after-trim` probe labels to `after-eviction`; inference outputs, counters, token counts and timings are unchanged. Timing fields are request-only observations excluding worker startup. There was no host-load gate, alternating control or OS page-cache flush, so median differences do not establish a latency/disk/NAND or memory improvement. Current next work is matched-history controlled performance and model-format adherence. The older six-turn and current nine-turn scores use different suites/limits and are not a quality-improvement comparison.

Worker/executable hashes matched before/after. The original private30-minute certificate was independently reverified afterward with its original model/runtime/cache settings, retaining identity `b66ba5cf0516500a810e58d8c692e6a22daf7bd31776d70e0c8ab9c8eb7abf0f`. This workload used separate disposable stores in qualification mode and issued no new evidence or ordinary-settings changes. After every owned worker stopped,33 captured generated state files totaling1,029,022,566 logical bytes were removed from this run's conversation/fresh stores; logs and reports remain. The certified cache was untouched. [Full workload and derived assessment](evaluation/ssd-near-limit-multiturn-20261106.json).

Reproduce with a fresh output directory (the selected runtime must match the worker source guards):

```sh
/path/to/selected/python scripts/validate-prefix-multiturn.py \
  --python /path/to/selected/python --model /absolute/gemma-2-2b-it-4bit \
  --store /absolute/Soliton/target/debug/examples/prefix-store-evict \
  --output-dir /absolute/new-workload-directory \
  --auto-eviction --near-limit --quota-mib 256
python3 scripts/prefix-multiturn.test.py
```

Legacy six-turn/manual-trim mode remains available with `--maintain /absolute/prefix-maintain`. Fresh stores can be large; the tool retains them for inspection, and the cleanup above was limited to this completed run's captured receipts. No native-window, other-model/Mac or broader-context validation was added.
