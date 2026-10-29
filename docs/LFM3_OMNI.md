# LFM3-Omni-2B integration

Full requested scope: text/image/audio/video input; text/speech/interleaved output, including realtime voice. **Full Omni support is unfinished. Media-input/text-output has passed qualification and shared API tests; speech/Realtime and GUI enablement remain gated.**

Official sources: [BF16 model](https://huggingface.co/LiquidAI/LFM3-Omni-2B), [GGUF runtime instructions](https://huggingface.co/LiquidAI/LFM3-Omni-2B-GGUF). Use the official GGUF path on Apple Silicon, rather than treating this as an existing MLX-LM text architecture. No auto-execution of Transformers remote code and no generic compatibility exception. The configuration advertises128000 text positions while the model card states32K for this checkpoint; neither is a locally qualified Soliton limit.

Pinned assets live under `.soliton-runtime/lfm3-omni-2b`; runtime under `.soliton-runtime/llama-b11960`. `scripts/prepare-lfm3-omni.py` rechecks release/weight/projector hashes. Model revision2918dd47cfceb4f260cd1734cd1aac1b22bbe4b6; runtime b11960/8642541b2. Original upstream server supports input_audio/input_video and realtime; video uses ffmpeg/ffprobe. Input audio16 kHz and output speech24 kHz. Reasoning is disabled.

`omni.rs` defines Rust-owned request contracts with opaque UUID media references, input/output modes and checked budgets. None/unknown media expansion refuses; text tokenizer estimates must never replace processor-aware counts for images, soundtrack, frames or audio. This admission helper is a contract only, not an inference route or proof that reservation semantics are validated for interleaved codebooks. `/studio/omni/capabilities` requires normal Studio authorization and explicitly reports qualified=false/runnable=false. New six tests and all77 core tests pass. Media bytes must remain on the Rust/backend side; WebView receives commands, asset IDs and bounded playback state.

The synthetic official-runtime smoke uses a red image/video, locally synthesized English audio and short TTS. It checks server-reported token usage but does not establish exact Soliton pre-admission parity. Text/image/video outputs matched the narrow probes. Audio contained42 but transcription was inaccurate, so ASR quality is not approved. Speech output was a nonempty24 kHz WAV; intelligibility, languages and realtime/interleaving are not approved. [Raw results](evaluation/lfm3-omni-runtime-smoke-20261029.json), [readiness](evaluation/lfm3-omni-readiness-20261029.json). The smoke server and TTS run sequentially and their owned process groups are stopped. Other project processes, including idle Ollama, were left untouched. No timing improvement is claimed.

Next: bind the runtime to the existing exclusive lifecycle and shared Document/history/telemetry, add Rust media import and exact processor admission, implement attachment/microphone/playback and realtime transport, then full quality/boundary/cancellation/identity qualification. Do not enable an engine simply because its protocol resembles OpenAI. Existing Gemma settings, history and qualification remain unchanged.

日本語: 音声出力・動画を含む全機能が対象ですが、Soliton内の対応は未完了です。公式ランタイムの起動試験とRustの基盤を実装しました。音声認識品質・Realtime・UI・正確なメディア事前計数・完全認証が終わるまで有効化しません。

简体中文：目标包括语音输出和视频等全部功能，但Soliton集成尚未完成。已实现官方运行时启动测试和Rust基础契约。在语音质量、Realtime、UI、精确媒体预计数和完整认证通过前不启用。


## Shared Rust bridge implementation — 2026-10-29

`lfm3Omni` is a new engine selection in the existing shared lifecycle. It uses the same owned Process, startup/stop/signal cleanup and active-request switching guard. Settings live in `settings.omni`: absolute `runtimeDir`, `model`, `projector`, `mediaRoot`, `evidence`, context1–4096. Normal Strata/vLLM settings are unchanged. Starting Omni requires exact pinned b11960 runtime-tree/Q4_K_M/Q8_0 hashes and fresh local `omni_media_text_30min` evidence with ≥1800s/zero failures/clean shutdown/all four count parities. Identity additionally binds OS, Mac hardware, decoder binaries, context/concurrency and reasoning/admission policy. Inherited LLAMA/GGML flags are removed; private engine credentials stay in the environment. The gate does not accept a short smoke report.

Authenticated metadata-only APIs: `POST /studio/omni/assets/import` receives an absolute user-selected file path and kind(image/audio/video), returning an opaque UUID. Rust copies at most128 MiB per file into private media storage (2 GiB quota), retaining the copy independently of source changes. Chat `Message.attachments` persists `{id,kind}` through the existing Document/conversation library. No media bytes or GPU frames enter normal snapshots. Native local URLs must be **relative IDs** (`file://UUID` under `--media-path`), not absolute file URLs. The first pre-soak attempt correctly failed on this integration error; the failed report remains intact.

`POST /studio/omni/count` counts the active common history plus the pending user message, with identical forced-text rendering to `/studio/chat`. The private native `/v1/chat/completions/input_tokens` route uses the same `oaicompat_chat_params_parse` and `process_mtmd_prompt(...).size()` as generation. Raw text tokenization is not substituted for media expansion. Count JSON is bounded64 KiB and must contain a positive integer; bad/unavailable responses latch refusal, and only a successful new engine launch clears it. Context/refused media remain request errors. Counting precedes history mutation; media processing/inference is serial. Raw API callers must also use Rust asset IDs and cannot bypass through other message/audio/realtime endpoints.

Native parity probes passed text28/image100/audio52/video109 input tokens against inference usage. Using the English Samantha voice corrected the earlier fixture issue; recognized text is now “The secret number is42.” These are narrow native fixtures, not broad ASR quality or Soliton certification. Full attempt2 JSON/SSE media probes precede the running1800-second soak; a separate isolated common-API test waits for successful evidence before launching any model. Normal settings will not change automatically.85 core tests and the server build pass, including counter-fault latch, history archive/restore, missing evidence rejection and shared switch ownership.

Speech, interleaved output and realtime remain explicitly refused by the shared text-output bridge, despite the official standalone runtime's audio-output smoke. UI/media playback and accurate output-codebook reservations remain next work; the full requested Omni scope is not complete. No MLX/Gemma compatibility check or existing count/exclusivity guard was removed.

日本語: 排他ランタイム接続、Rust管理メディア、共通履歴への参照保存、実処理済み事前計数を実装しました。ネイティブの4入力で計数一致を確認済みですが、30分認証とSoliton実モデル検証は進行中です。音声出力・Realtime・UIは未完了で、通常設定は変更していません。

简体中文：已实现排他运行时连接、Rust媒体存储、公共历史引用和实际处理后的预计数。原生4类输入的计数一致性已确认，30分钟认证及Soliton实模型验证仍在进行。语音输出、Realtime和UI尚未完成，普通配置保持不变。


## Media/text qualification completed; speech transport started

Full qualification passed1800.120239917 seconds,10072 successes,zero failures,clean shutdown and all native JSON/SSE media parity checks. [Byte-identical archived evidence](evaluation/lfm3-omni-media-text-qualification-20261029.json). The unchanged evidence hash matches the [real common-API report](evaluation/lfm3-omni-soliton-20261029.json): text/image/audio/video pre-counts25/94/60/103 each equal actual usage; exact4096 reservations accepted and4097 refused before history mutation. History attachments, exclusive switch, wrong-identity startup refusal and unchanged ordinary settings/evidence pass. Counts are workload-specific, not model constants; earlier28/100/52/109 probes use different prompts.

Speech/Realtime is a distinct scope. `omni_realtime.rs` collects bounded native text/audio events and stores PCM16 as private24 kHz WAV files on the Rust side. It never serializes audio frame payloads to the WebView. A text-only certificate cannot authorize the new scope; the same identity/time/stability checks plus exact input count, output reservation, cancellation, history and sample-rate checks are required. A qualification-only Rust WebSocket client and Python owner controller now probe native text/audio/interleaved modes. They do not enable an application route.

Native Realtime may render a different template and count audio codebooks differently. Chat pre-count equality is being measured, not presumed. Output reservation semantics must be proven before public admission, and realtime media history requires further work. The existing shared route continues to reject speech/realtime requests.90 core tests pass; new runtime dependencies are preserved alongside independent dependency updates, and a binary-hash-bound repeat of common API validation is queued after the speech probe stops. Normal user configuration and conversations are unchanged.

日本語: メディア入力・テキスト応答の30分認証と共通API実モデル検証が合格しました。音声出力・Realtimeは別認証で扱い、Rustの有界受信・音声保存と実験クライアントを追加しました。出力予算と計数一致が未確認のため、公開経路は未有効化です。

简体中文：媒体输入及文本输出的30分钟认证和公共API实模型验证已通过。语音输出及Realtime采用独立认证；已添加Rust有界接收、音频存储与实验客户端。输出预算及计数一致性仍待验证，公开路径尚未启用。
