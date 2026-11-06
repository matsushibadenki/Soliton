# Knowledge Lab

The first implementation prioritizes the requested knowledge addition, suppression and evaluation workflow. It creates a real MLX LoRA/QLoRA adapter, evaluates its saved weights, and retains the base model. It is not an implementation of gradient-free Weight-Space Programming or arbitrary factual erasure. The [referenced research](https://rikiyat.github.io/blog/weight-space-programming/) motivates controlled editing and evaluation; its reported capability improvements are not claimed for this application. The local training interface follows [MLX-LM's LoRA workflow](https://github.com/ml-explore/mlx-lm/blob/main/mlx_lm/LORA.md).

## Workflow

1. Open **Knowledge Lab → Edit**. Choose an absolute local MLX model directory containing `config.json`, tokenizer assets and Safetensors weights. Choose the Python executable from a compatible environment containing MLX-LM training dependencies. The existing vLLM executable's neighboring Python is suggested where available; suggestions are not compatibility certification. GGUF and remote model IDs are refused.
2. Name the experiment and record a source or editing reason. Supply 1–64 question/answer training examples. For suppression, the desired answer should be a refusal or other intended safe response, not the information to suppress.
3. Supply 4–32 independent evaluation questions spanning target, paraphrase, indirect and locality. Label each question language (English/Japanese/Simplified Chinese or unspecified). Choose exact (new default) or substring matching, an expected answer/phrase and up to eight explicitly approved alternatives. Suppression target/paraphrase/indirect cases also need a forbidden phrase. Prompt duplicates after whitespace/case normalization are rejected against training and other evaluation questions. This catches normalized literal leakage, not semantic overlap.
4. Stop inference and run **Train & evaluate**. Default settings are 20 iterations, four adapter layers, learning rate 0.0001, batch size one, rank eight, 512 training/prompt tokens and 128 output tokens. The UI permits bounded overrides. Model memory fit is not guaranteed; use an appropriately sized model. No dependencies or model weights are downloaded automatically.
5. Review the paired answers. Greedy decoding and seed zero are used in both runs. Exact matching ignores case and outer whitespace only: punctuation, units, internal whitespace and negation remain significant. Substring matching is a weaker explicit option. Only declared alternatives can satisfy a rule; translations/synonyms are not inferred. A forbidden phrase always rejects. Empty and truncated answers fail. All candidate checks and baseline locality checks must pass. These are deterministic literal rules, not a semantic judge or a general-capability benchmark.
6. Passing current-version experiments can be marked **Keep passing adapter**. Retention rechecks raw answers against the immutable experiment rules and requires indirect coverage; stored `passed` booleans alone are insufficient. The directory shown contains the artifact for use with MLX-LM's `adapter_path` option. Retention does not install the adapter in Soliton's production inference engine or fuse weights. A failed candidate remains available for inspection, but cannot be marked retained. Archiving hides nothing and removes no artifact. Export the report using the native save dialog or browser download.

## State and execution

- `Document.knowledge` stores the shared draft revision, view, selected experiment and up to 40 experiment records. A stale draft save is rejected. Unsaved form edits are temporary UI input. View changes and application reloads do not mutate inference conversations.
- `Document.workspaceMode` selects the top-level screen. Existing Chat/Logs docking remains available on the Inference screen; native workspace windows also render the same Knowledge Lab document.
- Authenticated `GET/POST /studio/knowledge` provides snapshots and `view`, `save`, `run`, `cancel`, `select`, `retain`, `archive` commands. Knowledge reports are fetched only on the active Knowledge Lab surface, not included in the main inference snapshot.
- Rust reserves the engine lifecycle before enqueueing a job; engine startup and model preparation cannot overlap a knowledge experiment. The worker receives an immutable experiment snapshot. Ordinary appearance changes remain possible during a job.
- Python is a supervised local numerical worker, necessary for MLX model training. Rust owns document state, job control, scoring, cancellation, persistence and artifact locations. No weight tensors, GPU frames or activation arrays are passed through the WebView. This screen only renders bounded scalar metrics and text; it does not implement a heavy visualization requiring wgpu.
- Baseline generation precedes training. Only training examples enter `train.jsonl`; held-out examples never enter training or a validation-driven optimizer. MLX freezes base parameters and trains LoRA parameters. The saved adapter is reloaded before candidate evaluation.
- Source metadata, tokenizer assets and Safetensors content are hashed before/after the experiment. Changed source files invalidate the result. Non-finite training loss stops the worker; non-finite adapter weights invalidate the result. The report records model and adapter hashes plus MLX/MLX-LM versions.
- The worker uses a separate process group, offline Hugging Face settings and `trust_remote_code=False`. It has a one-hour deadline. Cancellation kills/reaps the group; app shutdown waits for cleanup. Restarted in-flight jobs become interrupted and cannot be retained.
- The state file's parent contains `.soliton-knowledge/<UUID>/manifest.json`, `worker.py`, `data/train.jsonl`, `adapter/`, and `report.json`. Experiment folders are owner-only on Unix. Partial/failed artifacts remain for inspection. They are not automatically applied or considered usable. A report records the completion verdict; later retention/archive choices are stored in the shared document.

## Validation

- Rust tests cover held-out/category rules, suppression checks, truncation rejection, bounds, locality gates, stale-window saves, cancellation/process cleanup, reservation release and persistence.
- Real Apple Silicon / MLX 0.32.2 / MLX-LM 0.32.0 / Gemma 2 2B 4bit smoke: baseline → one-step adapter training → saved-adapter reload → three candidate answers → unchanged source hash. Target/paraphrase checks failed and locality passed, correctly yielding a rejected candidate. This establishes execution, not knowledge-edit efficacy.
- An aggressive 0.001 learning-rate experiment produced non-finite loss, motivating the fail-fast training guard. Its final rerun stopped at iteration 11/30, recorded failure and produced no candidate evaluation or acceptance fingerprint. No successful knowledge deletion or generalization is inferred from this test.
- Final real-model experiments used 30 steps, two adapter layers and learning rate 0.0001. Addition changed both held-out answers about a fictional beacon to the intended "Amber."; suppression changed both held-out France-capital answers from "Paris" to "I cannot share that.". Both retained the locality answer "4". Each suite passed 3/3 candidate checks, versus 1/3 at baseline, with unchanged source SHA-256. This is evidence for these small English fixtures only, not broad knowledge generalization, complete erasure or general-capability preservation.
- Browser checks exercise real report rendering, rejection gates, save/reload, shared draft and mode across two views, history selection, keyboard navigation, unchanged conversations, and English/Japanese/Chinese at 1280/390/320px in light/dark themes. Native Japanese report display, save/cancel dialog and floating/docking round trip were subsequently verified for the rubric-v1 report below. Other Macs remain pending.

## Remaining scope

Autonomous source verification, semantic/behavioral judges, actual unlearning, ROME/MEMIT, compression, weight heatmaps and automatic candidate activation are not implemented. LoRA response suppression leaves the original model intact and does not prove that the adapted model cannot recover a fact through other prompts. The local training worker does not send model weights to external services. The optional planner sends the explicitly entered goal and source material to OpenAI.


## OpenAI editing planner

Open **Edit → OpenAI editing planner**. Save any draft changes, enter your goal and source text (including unrelated facts for locality checks), select an OpenAI model supporting Structured Outputs, then generate. The default model field is `gpt-4.1`; access depends on your API project. Supply a session-only API key or set `OPENAI_API_KEY` in the Rust server environment. Keys are not persisted or returned in snapshots. The desktop app must inherit that environment if using the variable.

The Rust server calls the fixed HTTPS Responses endpoint with strict JSON Schema and `store:false`, a 120-second timeout and a bounded response. Only the entered goal, sources, operation and language are sent. API charges apply. Local model paths, weights, existing conversations and existing training examples are not included. Provider refusals, incomplete replies, malformed data and HTTP errors leave the draft intact. Generated examples are checked by the existing held-out and phrase-consistency validators.

Review the plan, risks, training examples and evaluation cases, then click **Review & import into draft**. Import preserves local model, Python and training settings. It does not start training. The generated plan, source text and producing model are persisted in the shared workspace and copied into the imported draft as planning provenance, so experiment manifests and reports retain the original proposal even after a later plan is generated. Draft revision and plan ID checks prevent stale imports. Source claims are user-reviewed; the planner does not browse or independently verify facts. Deletion remains response suppression. Planning and local worker operation have different data flows; this feature adds explicit external API communication.

Validation: plan/schema parsing, refusal/incomplete handling, loopback HTTP success/authentication-error fixtures, import/version guards, suppression/leakage validation and runtime-setting/provenance preservation tests pass (11 knowledge-related Rust tests). Live OpenAI generation is pending because this development environment has no API key.

API shape follows the [official Structured Outputs documentation](https://developers.openai.com/api/docs/guides/structured-outputs?api-mode=responses).


## Assessment rubric v1 — 2026-11-04

`assessmentVersion:1` is recorded on new experiments/manifests/reports. Each evaluation case stores `matchMode` (`exact` or `contains`), `aliases` (up to 8) and `language` (`en`, `ja`, `zh`, or `und` for unspecified). New default drafts include target/paraphrase/indirect/locality cases. Indirect questions test completion, implication or alternate routes to the edited fact; adding a category does not guarantee comprehensive extraction coverage.

Each measured answer records localized reason codes: `empty`, `truncated`, `notExact`, `missingExpected`, `forbiddenPresent`. The same Rust rules govern baseline/candidate scoring and retention. UI editing, planner preview, shared saves, reports and exports preserve the criteria and language. Prompt duplicates now also normalize internal whitespace. Required/alternative answers may not conflict with a forbidden phrase.

Legacy cases missing rule metadata retain substring behavior, empty aliases and unspecified language. Legacy reports missing `assessmentVersion` are version 0; their original status/artifacts remain visible. They cannot be newly retained under v1. Old three-category drafts can be opened/saved, but need an independent indirect question before another run. Existing retained records are historical results, not silently upgraded certification. Legacy generated plans missing required coverage must be regenerated before import; previously imported drafts remain editable.

The planner's strict schema now explicitly requires rule, aliases and language fields. Generated replies missing them are rejected instead of falling back to legacy behavior. Instructions request all four probe categories and multilingual evaluation when supported by supplied material. The requested generation model, endpoint, source-only payload, session-only keys, `store:false`, user review/import and no-auto-training behavior are preserved. Schema design follows the [official OpenAI Structured Outputs documentation](https://developers.openai.com/api/docs/guides/structured-outputs?api-mode=responses). Live generation remains pending an API key; local protocol/schema/import/refusal tests passed.

### Real-model check

Existing Gemma 2 2B 4bit / MLX 0.32.2 / MLX-LM 0.32.0 trained a separate two-layer adapter for 30 iterations at 0.0001 on four synthetic Q7-beacon examples. Twelve held-out tests covered four categories × EN/JA/ZH, with 16 output tokens per answer. Both baseline and saved/reloaded candidate completed all 12 cases. Source SHA-256 remained `3e8d02db8dc0df26d4e45291ae3a11ba9538671fc5337b22a8176f611a24b800`; losses and adapter weights were finite.

| Category | Baseline passed | Candidate passed |
| --- | ---: | ---: |
| Target | 0/3 | 1/3 |
| Paraphrase | 0/3 | 1/3 |
| Indirect | 0/3 | 0/3 |
| Locality | 3/3 | 1/3 |
| Total | 3/12 | 3/12 |

The English target/paraphrase/locality subset passed 3/3 after training, but the full suite correctly **rejected** the adapter and the retain API refused it. This exposes incomplete multilingual/indirect generalization and degraded Japanese/Chinese locality that the narrow English subset would miss. It is validation of rejection/gating, not evidence that this training configuration edits multilingual knowledge successfully. No adapter was activated or fused into normal inference.

Rust 144 tests, frontend/native builds and EN/JA/ZH 1280/390/320px browser checks passed. The browser test uses real Rust APIs and the actual report; only the legacy-report visual case is an explicit fixture. It verifies rules/aliases/language saves across two views, four category summaries, translated reasons, legacy retention blocking and metadata-preserving JSON export. Browser skill was not listed, so existing Playwright/Chrome was used. Native CUA additionally verified Japanese actual-report display, decoded JSON equality, cancel without a new file, and floating → edit → dock → evaluation with unchanged report/conversations.

[Raw replies, criteria, category counts and validation](evaluation/knowledge-rubric-v1-20261104.json). The local test never sends model weights or evaluation answers to OpenAI. Semantic truth, untested paraphrases, complete suppression/erasure, broader model quality and other hardware remain outside this evidence.

To reproduce on a freshly started, explicitly isolated `studio-server` (empty experiment history, stopped inference, private `STUDIO_STATE_PATH`), using an existing compatible local model/Python:

```sh
node scripts/knowledge-rubric-real.mjs --isolated \
  --api http://127.0.0.1:8081 \
  --model /absolute/path/to/gemma-2-2b-it-4bit --python /absolute/path/to/mlx/python \
  --output /tmp/knowledge-rubric-real-report.json
# With the frontend at 127.0.0.1:5180 and that completed QA fixture:
node scripts/knowledge-rubric-ui.mjs --isolated --api http://127.0.0.1:8081
```

The real test's successful execution can produce a **rejected** candidate: the purpose is to verify assessment, source integrity and retention gating, not force training to pass. The fixture script cancels only its own active experiment on failure/Ctrl-C. The browser test refuses ordinary/nonmatching QA state, uses a temporary legacy visual fixture and restores its draft/settings. Its JSON downloads remain local.
