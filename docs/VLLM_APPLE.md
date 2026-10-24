# vLLM-Apple: verified local startup

Latest environment proof / 最新の環境証跡 / 最新环境证据: [2026-10-24 environment refresh and cache diagnostics](#2026-10-24-cache-diagnostics-and-environment-refresh).

Current measured profile / 現在の検証済み設定 / 当前已验证配置: [4,096 total tokens, concurrency one](#2026-10-24-qualified-4096-context). Earlier dated entries and the 1,024 GUI fixture below are retained as history.

## 日本語

SolitonはvLLM-Appleの互換性検査を無効にしません。通常のMLX-LM version matrixは維持し、範囲外の環境では、vLLM-Appleが既に実装しているidentity-bound evidenceを明示指定します。証跡はモデル・バックエンドの実ファイル・runtime source・対象環境変数・Mac/OSを照合し、7日で失効します。コンテキストと同時要求数も取得時の上限以内に制限されます。

環境設定 → エンジン → vLLM-Appleで、Python・モデル・バックエンド実行ファイル・検証証跡JSON・コンテキストを指定します。MLX LMを選択してください。証跡を空欄にすると通常の互換性判定を使います。Solitonは`--architecture-evidence`と`--max-model-len`を渡し、証跡付き起動ではtuning middlewareを無効にします。`--skip-backend-check`や`--skip-runtime-probes`は渡しません。証跡の内容をSolitonが書き換えたり、期限を延長したりする処理もありません。

「モデルと証跡を診断」のアイコンから、停止中にモデル構造・証跡の一致・概算常駐メモリを確認できます。モデルをロードせず、診断結果はRust側で全ウインドウに共有し、設定変更または5分で非表示になります。「証跡一致」は確認時点の限定された検証範囲です。起動時は必ず再検査します。理由は情報アイコンに表示し、Pythonの生ログやファイル内容をUIへ渡しません。

証跡付きMLXのGUI/API推論では、Solitonがバックエンド隣のPythonでローカルtokenizerとチャットテンプレートを読み、入力＋要求出力の正確なトークン数を事前判定します。重み・ネットワーク・remote codeは使いません。上限超過は422 `context_length_exceeded`、計数できない場合は503 `tokenization_unavailable`として拒否し、会話には追記しません。APIは正の`max_tokens`を明示してください。現在はplain textのchat/completionsとcompletions、1要求を対象とし、tools・独自テンプレート・複数候補は拒否します。計数専用CPUプロセスは選択中のエンジンに紐づけて1つだけ再利用し、tokenizerは初回に読み込みます。要求本文・回答・計数結果はキャッシュせず、停止・切り替え・設定変更でプロセスも終了します。ファイル変更や計数異常後は拒否状態を維持し、停止・再起動で互換性を再検査するまで再利用しません。別のモデルエンジンは常駐しません。

新規証跡の取得例（現在の環境に対応する値を指定）：

```sh
cd vLLM-apple
.venv/bin/python -m vllm_apple qualify-model "$PWD/models/gemma-2-2b-it-4bit" \
  --backend-kind mlx_lm \
  --backend-executable /opt/homebrew/opt/vllm-metal/libexec/bin/mlx_lm.server \
  --candidate-mlx-lm-version 0.32.0 \
  --max-model-len 1024 --concurrency 1 --duration 1800 --warmup 5 \
  --phase-samples 3 --bind-architecture-evidence \
  --backend-port 19231 --output "$PWD/qualification-results/soliton-evidence.json"
```

`--candidate-mlx-lm-version`は検証対象をインストール済み版へ厳密に固定する、qualification専用の既存オプションです。通常serveの許可範囲を広げるものではありません。証跡の発行には実モデルのロード、stream/non-streamの比較、三言語の短い品質確認、phase計測3回、1800秒以上の失敗なしのsoak、RSS増加上限、正常終了、実行前後のidentity一致が必要です。失敗・短縮・期限切れの結果を起動用証跡へ昇格させません。

再現検証：`npm run build`と`cargo build -p studio-server`の後、プロジェクトルートで次を実行します。専用ポート18082と一時保存データを使用します。既存のGPU推論プロセスは先に停止してください。

```sh
SOLITON_REAL_EVIDENCE=/absolute/path/to/soliton-evidence.json \
PLAYWRIGHT_CHROMIUM_EXECUTABLE='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' \
npm run test:real-model
```

この検証は証跡なし・期限切れ・コンテキスト上限超過の拒否、実daemonのinference-ready、三言語GUI生成、OpenAI API、キャンセル、子プロセス停止、Strata mockとの切り替え、再起動後の履歴を確認します。Strata mockの切り替え確認はStrata実モデルの品質試験ではありません。`SOLITON_REAL_NATIVE=1`を付けると、ポート8080が空いている場合にビルド済みTauriアプリ本体を起動し、同じ試験を実施します。ネイティブ版も`STUDIO_STATE_PATH`で一時保存先を指定でき、通常の保存先や会話は変更しません。画面操作はそのネイティブruntimeに接続したブラウザーから行います。既定reportは`/tmp/soliton-real-model-report.json`です。別のモデル・Python・実行ファイル・report先は`SOLITON_REAL_MODEL`、`SOLITON_REAL_PYTHON`、`SOLITON_REAL_EXECUTABLE`、`SOLITON_REAL_REPORT`で明示できます。

認定範囲はそのMac・環境・モデルの短いtext workloadです。長文品質、全モデル、並行batch、他SoC、24時間安定性、性能優位を認定するものではありません。証跡失効や依存更新後は再取得が必要です。

## English

Soliton preserves the normal version matrix and all daemon validation. Optional identity-bound qualification evidence authorizes only the measured model/backend/runtime/environment/Mac, expires after seven days, and caps context and concurrency. Settings expose the evidence JSON and explicit context limit. Launch uses `--architecture-evidence`, `--max-model-len` and, when evidence is supplied, `--disable-metal-tuning`; no `--skip-*` option is added.

The stopped-engine inspection icon checks local metadata, evidence identity and estimated resident memory without loading weights. Rust shares a sanitized result across windows for up to five minutes, invalidated by settings edits. Startup always revalidates. Hover the information icon for details.

Qualified MLX GUI/API requests also count prompt-template tokens locally with the backend environment's tokenizer before inference. Prompt plus explicit positive `max_tokens` must fit the selected context. Overflow returns 422 `context_length_exceeded`; unavailable counting returns 503 `tokenization_unavailable`, with no conversation mutation. The supported scope is plain text chat/completions or completions, one candidate, no tools/custom templates. Counting loads no weights, uses no network/remote code, and reuses one CPU-only counter for the selected engine session after its first load. Requests, answers and counts are not cached. Stop, engine switch and settings edits reap the counter. File changes or count/protocol failures reject until an explicit stop/restart revalidates compatibility. Upstream startup checks remain intact.

Use the existing `qualify-model --candidate-mlx-lm-version ... --bind-architecture-evidence` workflow above for a fresh, full 30-minute result. Candidate selection is qualification-only, requires the exact installed version, and does not broaden normal startup compatibility. Failed or abbreviated results cannot mint evidence. Run `npm run test:real-model` with `SOLITON_REAL_EVIDENCE` for isolated real-model GUI/API and rejection tests. Scope is the measured short text workload, not universal model or performance certification. Requalify after expiry or dependency changes.

## 简体中文

Soliton保留原有版本矩阵和daemon检查。可选的identity-bound证据仅允许已验证的模型、后端、runtime、环境和Mac，七天失效，并限制上下文及并发数。设置提供证据JSON和上下文上限。启动传递`--architecture-evidence`、`--max-model-len`，使用证据时关闭tuning middleware，不添加任何`--skip-*`参数。

停止引擎后可通过诊断图标检查本地元数据、证据及估算常驻内存，无需加载模型。Rust在各窗口共享经过整理的结果，设置更改或五分钟后失效；每次启动仍会重新检查。原因显示在信息图标的提示中。

使用证据的MLX GUI/API请求会先用后端环境中的本地tokenizer计算聊天模板及输入长度，加上显式的正数`max_tokens`必须在上下文上限内。超限返回422 `context_length_exceeded`，无法计数则返回503 `tokenization_unavailable`，不修改会话。当前支持单个候选的纯文本chat/completions和completions，不支持tools或自定义模板。不加载权重、不联网、不执行remote code；计数用CPU进程仅在选定引擎会话内复用一个，首次加载tokenizer；不缓存请求、回答或计数结果。停止、切换引擎及设置更改会结束进程。文件变化或计数异常后保持拒绝，需停止并重启引擎以重新验证兼容性。原有启动检查保持启用。

请使用上面的`qualify-model --candidate-mlx-lm-version ... --bind-architecture-evidence`流程取得完整30分钟的新证据。candidate选项仅用于严格匹配已安装版本的验证，不放宽正常启动条件。失败或缩短的验证不能生成证据。设置`SOLITON_REAL_EVIDENCE`后运行`npm run test:real-model`，验证隔离的真实模型GUI/API及拒绝路径。验证范围为当前环境的短文本工作负载，不代表所有模型或性能认证。证据失效或依赖更新后需重新验证。

## 2026-10-21 result / 実測結果 / 实测结果

🟢 [Done] Apple M4 / 32 GiB, MLX 0.32.1, MLX-LM 0.32.0, Transformers 5.19.0, Gemma 2 2B 4-bit: 1800.568 s, 3137/3137 successful requests, zero failures, 15,450,112 bytes peak RSS growth (limit 268,435,456). Native Soliton runtime passed three-language GUI inference, non-stream API, cancellation, worker shutdown, Strata-mock switching and persistence. Evidence rejection cases also passed. [Recorded result](evaluation/soliton-vllm-apple-2026-10-21.json).

2026-10-21の証跡は履歴として保持しています。MLXが0.32.2へ更新された現在の環境ではidentity不一致となり、利用できません。最新の結果は下記を参照してください。


## 2026-10-22 requalification / 更新後の再検証 / 更新后重新验证

🟢 [Done] Apple M4 / 32 GiB, MLX 0.32.2, MLX-LM 0.32.0, Transformers 5.19.0, Gemma 2 2B 4-bit: 1800.506 seconds, 2894/2894 successful requests, zero failures, 11,829,248 bytes peak RSS growth, clean shutdown and unchanged identity. [Recorded result](evaluation/soliton-vllm-apple-2026-10-22.json). Desktop activity included application builds; timing differences from October 21 are not performance comparisons.

日本語: 旧証跡の環境不一致をネイティブ版の診断で確認し、モデルを起動せず、既存6メッセージを保持しました。新しいローカル証跡は`.soliton-runtime/gemma2-evidence-2026-10-22.json`です。有効期限は2026-10-29 18:16:20 JSTで、環境が変われば期限前でも無効です。コンテキスト1024・同時要求1のtext範囲を維持し、通常のversion matrixは変更していません。

English: The native diagnostic correctly rejects the old installation identity without starting a model or changing six existing messages. Fresh local evidence is `.soliton-runtime/gemma2-evidence-2026-10-22.json`, valid until October 29, 2026, 18:16:20 JST while identity remains unchanged. Scope stays text/context 1024/concurrency 1; the normal version matrix is unchanged.

简体中文：原生诊断正确拒绝旧环境证据，不启动模型并保留原有六条消息。新证据位于`.soliton-runtime/gemma2-evidence-2026-10-22.json`，环境不变时有效至2026-10-29 18:16:20 JST。范围仍为文本、上下文1024、并发1，普通版本矩阵保持不变。


最終ネイティブ版は事前トークン判定を有効にした状態で、三言語GUI・API・上限超過拒否・キャンセル・子プロセス停止・Strata mockとの切り替え・再起動後の履歴を確認済みです。通常の保存先でも新証跡で起動・推論・停止し、既存6メッセージと設定を保持しました。通常起動用のローカル設定は最新証跡へ更新済みです。[多言語・上限付近の実測](BENCHMARKS.md)。

The final native bundle passed three-language GUI/API inference, exact-context rejection without history writes, cancellation, worker shutdown, Strata-mock switching and persistence. Ordinary startup also passed with the fresh evidence, preserving six messages and settings. Normal local settings now point at the fresh evidence. See the [workload measurements](BENCHMARKS.md).

最终原生版本已通过英日中文GUI/API、精确上下文拒绝且不修改历史、取消、子进程停止、Strata mock切换及重启持久化验证。普通保存路径也使用新证据完成启动、推理和停止，保留六条消息和设置；本地启动设置已更新。参见[工作负载测量](BENCHMARKS.md)。

### Lightweight qualified tokenizer / 検証済み計数の軽量化 / 已验证计数的轻量化

For the measured Gemma 2 files and dependency versions, the counter directly uses the original serialized tokenizer and sandboxed Jinja template, with exact file-hash/version selection. It does not import the inference libraries. Other models/configurations retain the original offline AutoTokenizer path; no compatibility gate is skipped. Full token-ID parity covers raw completion, Unicode, special tokens and multi-turn chat. See [measurements](BENCHMARKS.md#token-counter-first-response-and-memory--2026-10-22).

日本語: 検証済みGemma 2のファイルハッシュと依存版が一致した場合、元のtokenizerとJinjaテンプレートを直接使い、推論ライブラリの読み込みを省きます。それ以外は従来のオフラインAutoTokenizer経路です。互換性検査を省略せず、raw入力・Unicode・特殊トークン・複数ターンのトークンID一致を検証しています。

简体中文：当已验证Gemma 2的文件哈希及依赖版本匹配时，直接使用原始tokenizer和Jinja模板，避免加载推理库。其他情况保留原有离线AutoTokenizer路径。兼容性检查保持启用，并验证raw输入、Unicode、特殊token及多轮对话的完整token ID一致性。

Qualified counter preparation / 検査後の先行計数準備 / 检查后的提前计数准备：After authenticated control readiness, Rust starts the same CPU-only counter during model loading, without synthetic input. It retains tokenizer memory before first chat; exact counting and all startup gates remain mandatory. 起動検査後のモデル読込中に、同じCPU計数プロセスを先行起動します。入力は作らず、初回チャット前からtokenizer分のメモリを保持します。正確な計数とすべての起動検査は維持します。确认启动检查后的控制就绪状态后，于模型加载期间启动同一CPU计数进程，不构造输入；首次聊天前即保留tokenizer内存，准确计数及全部启动检查保持启用。See [results](BENCHMARKS.md#qualified-startup-overlap--2026-10-23).

## 2026-10-23 fingerprint requalification / 指紋計算改善後の再検証 / 指纹优化后的重新验证

The complete-content fingerprint now uses native strict macOS path resolution and bounded POSIX containment, preserving all hashing/mutation/inventory checks. Three paired trials: 9.353→6.219 s median, identical digests on all six scans. Fresh qualification: 1800.076 s, 5223/5223 successes, zero failures, clean shutdown; normal native/headless settings now select `.soliton-runtime/gemma2-evidence-2026-10-23-fingerprint.json`. Evidence is valid only for this exact runtime/environment/model/Mac and context 1024/concurrency one, until 2026-10-30 20:50:58 JST. Environment flags must match the measured launch; do not force an old identity or edit an old report. [Evidence and limits](BENCHMARKS.md#full-content-fingerprint-optimization--2026-10-23), [reproducible patch](../patches/README.md).

日本語: 全内容検査を維持した指紋計算を9.35→6.22秒へ短縮し、変更後の環境を30分・5223件成功・失敗0件で再検証しました。通常起動の証跡設定だけを更新し、履歴と他の設定は維持しています。環境変数やruntimeが変われば再検証が必要で、古い証跡の値は書き換えません。有効期限は同一条件で2026年10月30日20:50:58 JSTです。

简体中文：保留完整内容检查的指纹计算9.35→6.22秒，修改后的环境通过30分钟、5223次全部成功、零失败的重新验证。仅更新正常启动证据设置，保留历史及其他设置。环境变量或runtime变化后必须重新验证，不修改旧证据值。同一条件下有效期至2026年10月30日20:50:58 JST。


## 2026-10-24 qualified 4096 context

日本語: このM4・Gemma 2 2B 4-bit環境では、入力＋出力予約の合計4,096、同時実行1へ更新しました。証跡は`.soliton-runtime/gemma2-context4096-20261024.json`で、環境が一致する場合の有効期限は2026-10-31 12:30:03 JSTです。30分の短文混合試験と約10分の長文試験を通し、ネイティブ／headless設定のcontextと証跡パスだけを更新しました。会話・他の設定は保持しています。出力を512予約する場合、入力はテンプレート込みで3,584以下が必要です。4,096を超える設定、旧証跡での拡大、期限切れ・環境変更は従来どおり拒否します。新規環境の既定値は1,024のままで、他モデル／Macの上限を自動拡大しません。

English: Ordinary native/headless preferences now select the fresh 4,096-total-token/concurrency-one evidence for this exact M4/Gemma installation, expiring October 31, 2026, 12:30:03 JST if identity stays unchanged. Only context/evidence preferences changed, with original-byte backups. A 512-token output reservation leaves at most 3,584 input/template tokens. Full startup verification, exact CPU counting, failure latching and stop-before-switch stay mandatory. The 30-minute mixed short-text qualification and separate approximately ten-minute near-limit matrix are distinct measurements; 4,096 GUI interaction and other models/Macs are not certified by this headless expansion. New installations keep the conservative 1,024 default.

简体中文：此M4／Gemma环境已配置为输入加输出预留总量4096、并发1。新证据为`.soliton-runtime/gemma2-context4096-20261024.json`，环境一致时有效至2026-10-31 12:30:03 JST。通过30分钟短文本混合验证及另行约10分钟长文本矩阵后，只更新原生／headless配置的context和证据路径，并备份原始数据。预留512输出token时，输入加模板最多3584。继续执行完整启动校验、准确CPU计数、故障拒绝及切换前停止；超范围、过期或环境变更仍会被拒绝。新环境默认1024，不自动扩大其他模型／Mac的范围。

Requalification uses the earlier full command with `--max-model-len 4096`; do not shorten duration or skip checks. For the isolated long-context runner and guarded preference activation, see [procedure and results](BENCHMARKS.md#qualified-context-expansion--2026-10-24), [qualification](evaluation/soliton-vllm-apple-context4096-20261024.json) and [startup refusals](evaluation/context4096-startup-gates-20261024.json). The older GUI `test:real-model` fixture still requires 1,024 evidence; its earlier results do not certify GUI interactions at 4,096.


## 2026-10-24 cache diagnostics and environment refresh

日本語: 現在のログイン／実行環境では`PYTORCH_ENABLE_MPS_FALLBACK=1`が追加され、以前の証跡は環境不一致として正しく拒否されました。フラグを消して旧証跡を再利用せず、完全な30分再認証を実施しました。5,978件成功・失敗0件・正常終了を確認し、長文キャッシュ38件の正答、正確な上限判定、停止・復旧・排他切り替えも通過しました。通常のネイティブ／headless設定は証跡パスだけを`.soliton-runtime/gemma2-cache-memory-env-20261024.json`へ更新し、コンテキスト4,096・会話・他の設定を保持しました。有効期限は、同一環境で2026-10-31 18:31:45 JSTです。

English: The current login/tool environment adds `PYTORCH_ENABLE_MPS_FALLBACK=1`; no other installation identity changed. Old proof remains rejected instead of deleting the flag to fit old approval. Fresh complete qualification passed 5978 requests over 1800.675 seconds with zero failures and clean shutdown. The separate serial long-history workload, exact boundary, shared telemetry and exclusive cleanup/recovery passed. Only ordinary native/headless evidence paths were refreshed; context remains 4,096. The new evidence expires October 31, 2026, 18:31:45 JST with unchanged identity. Cache diagnostics do not impose a byte cap or claim reduced memory; measured idle physical footprint remained approximately 7.91 GiB.

简体中文：当前登录／工具环境增加`PYTORCH_ENABLE_MPS_FALLBACK=1`，其他安装身份未变。保留旧证据的拒绝结果，不通过删除变量绕过检查。完整30分钟再认证5978次成功、零失败、正常结束；长历史、精确边界、共享计测、互斥清理和恢复也通过。仅更新正常原生／headless证据路径，上下文仍为4096、会话和其他设置保持不变。新证据在身份不变时有效至2026-10-31 18:31:45 JST。内存诊断未设置字节上限，也不宣称降低内存；空闲后physical footprint仍约7.91 GiB。

[Measured cache behavior and limits](BENCHMARKS.md#diverse-long-history-cache-memory--2026-10-24). The refresh tool requires existing valid 4,096 evidence with identical non-environment identity, a passing cache workload and startup-refusal reports, stopped preference owners and the original full verifier. Its cache path cannot increase context or approve model/backend/runtime changes; those require the full context matrix.
