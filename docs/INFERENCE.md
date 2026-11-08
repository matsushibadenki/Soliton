# 推論内容とリクエスト計測

チャットの回答に「推論の内容」とツール呼び出しを折りたたみ表示する。Strataが返す `reasoning_content` と `tool_calls` を表示し、回答本文とは分けて保持する。ツール呼び出しの名前・引数は表示のみで、Studioが実行する機能ではない。推論文を返さないモデルでは推論欄は表示しない。

各回答の計測行を開くと、リクエストID・モデル・開始時刻・終了状態・所要時間・初回出力まで・回答開始まで・トークン数・エンジン速度・処理時間・終了理由を確認できる。

| 値 | 取得元 |
| --- | --- |
| 所要時間、初回出力・回答開始まで | Rustゲートウェイの単調時計。上流待機・通信・クライアントの受信待ちを含み、純粋なGPU実行時間ではない |
| 初回出力 | 最初の空でない回答・推論・ツール呼び出し。単なるrole通知やkeep-aliveは含まない |
| トークン数 | その応答の `usage`。チャンク数や文字数から推定しない |
| エンジン速度・プロンプト処理・デコード時間 | その応答の `timings`。出力トークン数÷所要時間で代用しない |
| 未取得の値 | 「—」。0は上流が0と報告した場合のみ |

「完了」「中断」「失敗」を区別する。通常の終了理由 `length` は完了扱いで、終了理由欄で確認できる。クライアントの中断が完了通知の受信後なら完了、通知前なら中断として記録する。不完全なストリームやHTTPエラーは失敗になる。アプリ中断後に読み込んだ生成途中の会話は中断へ戻し、最終所要時間は復元不能なので「—」とする。

環境設定 → APIの「リクエスト計測」には、直近100件の完了・中断・失敗したチャットと対応APIリクエストを表示する。OpenAI互換の `/v1/chat/completions` と `/v1/completions`、Anthropic互換の `/v1/messages` を対象とし、通常JSONとストリームの両形式を読む。モデル一覧の取得は含めない。モデルが返さないフィールドは表示しない。

計測一覧はRust共有State内だけに保持し、アプリ終了時にリセットする。APIリクエストの回答本文や推論文は一覧に保存しない。チャットでは推論文・ツール引数・計測値を会話本文と一緒に保存し、会話切り替え・再起動・JSON書き出しでも保持する。次のモデル入力には会話のrole/contentだけを送り、表示用の推論・計測データを混入させない。

保存には回答／推論各1 MiB、ツール呼び出し最大64件・各引数64 KiBの上限があり、省略した場合は明示する。会話ライブラリーの本文容量検査には推論文とツール引数も含める。履歴はRustが所有し、各WebViewは表示のみ行う。


## Kapitsaから取り入れた計測設計（2026-11-01）

参照版: Kapitsa `2d374d5e85bcc87708527102dee3ac7d40119dfe`（ローカル追加されたソース、Apache-2.0）。[計測API](https://github.com/nace-ai/kapitsa/blob/2d374d5e85bcc87708527102dee3ac7d40119dfe/src/api/chat_completions.cpp)、[SSDキャッシュ](https://github.com/nace-ai/kapitsa/blob/2d374d5e85bcc87708527102dee3ac7d40119dfe/src/engine/prefix_cache.h)、[履歴準備の優先制御](https://github.com/nace-ai/kapitsa/blob/2d374d5e85bcc87708527102dee3ac7d40119dfe/src/engine/history_gate.h)、[doctor](https://github.com/nace-ai/kapitsa/blob/2d374d5e85bcc87708527102dee3ac7d40119dfe/src/engine/doctor.cpp)を調査しました。

最初の採用は、キャッシュで省略できた入力と、新しく読み込んだ入力の処理速度を分ける設計です。計算・保存はRust共有Stateで行い、チャットの計測詳細とAPIの要求履歴に三言語で表示します。

| 表示 | 算出・条件 |
| --- | --- |
| 入力キャッシュ再利用率 | 応答のキャッシュ読込数 ÷ 入力総数 × 100。リクエストのヒット率や速度向上率ではありません |
| 未キャッシュ入力 | 入力総数 − キャッシュ読込数。またはエンジンが報告する`timings.prompt_n` |
| 入力処理速度 | 未キャッシュ入力 ÷ 応答の入力処理時間。完了要求に限定 |
| 生成速度 | 応答の`timings.predicted_per_second`。入力処理速度と別の表示 |
| キャッシュ書込トークン | Anthropic形式の`cache_creation_input_tokens`。読込分と区別 |

Kapitsaの`usage.kapitsa.prefill_ms`を入力処理時間として読めるようにしました。通常の`timings.prompt_ms`にも対応します。入力数とキャッシュ数がない場合、初回出力時間や総経過時間から入力処理速度を代用しません。全入力が再利用された場合、時間0、数値の不整合、中断・失敗では入力処理速度は「—」です。`prompt_n`と入力差分が一致しない場合も速度を表示しません。追加フィールドのない過去の会話は従来の値を保持します。

Anthropic形式の入力総数は、非キャッシュ入力・キャッシュ読込・キャッシュ書込の合計へ正規化しました。増分イベントを足し込み続けず、各カウンターの最新値を保持します。入力総数と出力数が揃えば合計も算出し、整数の桁あふれ時は未取得として扱います。[公式仕様のトークン内訳](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)に基づきます。

### 次の採用候補

| アイデア | Solitonへの取り込み方 | 進捗 |
| --- | --- | --- |
| キャッシュと処理段階を分けた計測 | Rustで集計し、既存の要求詳細へ表示 | 🟢 [Done] |
| `doctor`のモデル適合診断 | 重み・KV・作業用メモリ・OS予約・コンテキスト・空き容量をエンジン別に評価。推定と実機確認を区別 | 🟠 [Next] |
| SSD上の会話プレフィックス | 既存バックエンドの状態保存／復元APIを調査し、モデル・実行環境・状態形式の識別、整合性検査、容量制限を設計 | 🟠 [Next] |
| 静穏時の履歴準備 | 前面要求を優先し、新しい要求で譲る。古い会話準備を破棄し、キャッシュを圧迫しない制御 | 🔴 [Later] |
| KFlash・Neural Engine・C ABI SDK | Rust管理のワーカープロセスとの接続を検討。モデル対応・キャンセル・異常終了・実機品質を確認して段階導入 | 🔴 [Later] |
| 他Macでの測定・複数Mac連携 | 対象実機と接続先が必要 | ⭕️ [Pending] |

Kapitsa本体のエンジン追加、KVの保存形式変更、推論カーネルの高速化は今後の項目です。今回の検証はRustの応答パーサーと計算、および模擬データを使うブラウザー表示です。KapitsaやGPUでの高速化・実モデルの品質向上を示す測定ではありません。

再確認: `cargo test -p studio-core`、`npm run build`、`node scripts/prompt-metrics-ui-smoke.mjs`。UI試験はBrowser skillがないため既存Playwrightを使用し、三言語、1440/390 px、通常・欠損・中断、コンソールと横はみ出しを確認します。画像はシステム一時ディレクトリへ保存します。


## 共通モデル診断 — 2026-11-02

環境設定 → エンジンの「モデル診断」を開くと、Rust共有Stateが保存済み設定から作成した観測値を確認できます。設定の編集中は古い診断を隠します。GPU・Python・モデルの起動、ダウンロード、設定書換えは行いません。通常の起動・資格確認・入力受付の制限は維持します。

| 項目 | 取得元と範囲 |
| --- | --- |
| コンテキスト | Strata Metalは既存ポリシー適用後の次回値、Gemmaは構成引数、vLLM/Omniは保存済み設定。稼働中エンジンの実測値ではありません |
| モデルファイル | Gemmaの設定にある単一GGUFの論理ファイル容量。モデルの常駐量ではありません。他形式の再帰走査は行いません |
| 常駐推定 | vLLMの既存診断が設定一致・5分以内の場合のみ。実測ではなく、検証合格表示へ転用しません |
| OS予約・予算 | Metalは次回適用値、それ以外は既存Macポリシーを用いた計画値。搭載メモリから予約を引いた値で、現在の空きRAMやバックエンドによる強制確保ではありません |
| ディスク空き | Gemmaはモデルの配置ボリューム、MetalはStrata-data、vLLM/Omniはモデル配置先。新規作成先は最寄りの既存親で調査。準備に必要な全容量の判定は既存準備画面で行います |
| KV・作業用メモリ | 未取得。推測で埋めず「—」。したがって警告がなくても「部分的な確認」であり、適合とは判定しません |

OS予約が搭載量以上、モデルファイルが予算超過、最大出力がコンテキスト以上、空き容量0、設定／モデル欠落、バックエンド報告のメモリ不足を案内します。ファイル容量超過は読み込み方式の確認を促す警告であり、SSDストリーミングの利用可否を決めるものではありません。未取得ディスク値は容量0と区別します。診断の説明は情報アイコンにまとめています。

検証: `cargo test -p studio-core`、`npm run build`、`node scripts/model-plan-ui-smoke.mjs`。Rustでは設定変更への追従、欠落値、相対モデルパス、予約不足、不正・重複コンテキスト、設定ファイル不変を確認。Browser skillがないためPlaywrightで三言語・1440/390/320 pxの開閉、欠落値、警告、横はみ出し、コンソールを確認します。UI値は模擬データで、実機の推論性能や適合確認ではありません。

次はモデル／バックエンドごとのKV・作業用メモリと準備ディスク予算の根拠を追加し、その後にSSD状態保存APIの調査へ進めます。他Mac検証・署名・公証は保留を維持します。


## バックエンド別KV・作業用バッファ — 2026-11-02

共通モデル診断にバックエンド別の計算を追加しました。前節のKV・作業用メモリが一律未取得だった状態を更新します。KV／状態は `≈`、作業用バッファの既知分は `≥` と表示し、情報アイコンで対象範囲を確認できます。小さい値はMiB表示にして、1 MiBを0.00 GiBに丸めません。

| 対象 | KV／状態データの計算 | 作業用メモリの既知分 |
| --- | --- | --- |
| 同梱Strata Gemma 3 Metal | 固定llamaコミット、並列1、既定F16 KVに限定。GGUF v3ヘッダーのGemma 3 1B形状を検査。`26層 × K/V 2個 × 256次元 × 2 bytes × ceil(context/256)×256`。全層を全コンテキストで見積もり、SWAによる節約は差し引きません | llama-contextの出力ベクトル1つ、`vocab × 4 bytes`。計算グラフ・入力処理・Metal割当の全量ではありません |
| vLLM-Apple / mlx_lm | ローカルconfig.jsonのgemma2／gemma3_textのみ。`2 × layers × KV heads × head_dim × dtype bytes × ceil(context/256)×256`。明示されたFP16/BF16/FP32を使用。SWAの節約は差し引きません | 出力ベクトル1つのデータ分、`vocab × dtype bytes`。MLXの遅延評価・一時テンソル・prefill・アロケータを含む総量ではありません |
| Strata Metal Qwen window | 確認済みwindow.cppのSHA-256一致時のみ。QSA12層のint8 K/V・スケール・索引、GDN36層の状態と二重conv履歴、PLE履歴。コンテキストはソースと同じセル丸めを使用 | `R_`, `bo_`, `xn_`, `qkv_`, `z_`, `qkvL_`, `zL_`の必須バッファ。prompt GEMM、MTP、エキスパートarena、追加バッファは含みません |
| vllm_metal／LFM3-Omni／未確認レイアウト | 未取得。汎用の密Transformer式を流用しません | 未取得。特に画像・音声・動画の作業量をテキスト量から推測しません |

根拠: 同梱llama.cpp `3cf03257f219afbe7334045ff7c6a06ac68c627d` の `src/llama-kv-cache.cpp`, `src/llama-kv-cache-iswa.cpp`, `src/llama-context.cpp`、既存のGemma変換器、MLXの密注意KV形状と既存vLLMモデル検査の式、Strata `46c152a1310f77cd2f0ce2c7cd213962df14fe83` の `metal/src/engine/window.cpp`。Strataソースの対象SHA-256は `4895f2ba12e91d7893935e42cbd82f0ac1ea5c82fa9920923011593e777f9dfd` です。この一致は計算式を選ぶ条件で、バイナリの同一性・動作品質の証明ではありません。MLXのKV式はconfigの浮動小数点dtypeを仮定した計画値です。

保持KV上限とアロケータ空きキャッシュ上限は別の行に表示します。前者は使用中の全KV、後者は使用中のテンソル全体を制限する値ではありません。0／既定値は0 bytesとして表示せず未取得にします。これらの上限と常駐推定をKV計画値へ機械的に加算しません。重複や共有領域があるためです。

KV／状態の計画値＋既知作業用バッファがOS予約後のメモリ予算を超える場合は確認を促します。警告がなくても適合とは判定しません。既知の下限だけで全作業用メモリを賄えるとは判断できないため、状態表示は「部分的な確認」のままです。変更した設定・モデルメタデータは次の共有スナップショットで再評価し、解析失敗時に古い推定を残しません。GGUFの読取は先頭1 MiB以内、設定は1 MiB以内、Strataソースは512 KiB以内に制限し、テンソル本体・語彙配列を読み込まず、GPU/Pythonを起動しません。

### 確認方法と結果

`cargo test -p studio-core`（110件）、`npm run build`、`node scripts/model-plan-ui-smoke.mjs`。モデル形状、dtype、256単位丸め、過大値・不正GGUF、未対応レイアウト、設定変更後の未取得復帰、予算警告と上限分離を確認しました。三言語・1440/390/320 pxの開閉、欠落、警告、上限表示、横はみ出し、コンソールをPlaywrightで検査しました（Browser skillがないため既存Chrome経由）。

読み取り専用CLIも追加しました。

```sh
cargo run -p studio-core --example model-plan -- strata /path/to/runtime strata-gemma3-1b.json
cargo run -p studio-core --example model-plan -- mlx /path/to/local-model 4096
```

CLI例の搭載メモリは比較用に32 GiBを指定しています。実アプリは従来どおり取得した搭載量を使用します。取得済みGemma 3 1Bの実GGUFで、2048コンテキストのKV計画値52 MiB・出力ベクトル1 MiBを確認。元のHFメタデータをMLXプロファイルで診断すると4096で104 MiB・0.5 MiBでした。これはMLXでの当該モデルの起動可否や品質確認ではありません。確認済みStrataソースと一時的なMetal設定では8192で状態データ240,715,776 bytes・既知作業バッファ20,135,936 bytesでした。元のモデル・設定は変更していません。

実GGUFのRust診断JSONを `SOLITON_MODEL_PLAN_FIXTURE=/path/to/report.json node scripts/model-plan-ui-smoke.mjs` でGUIへ渡す経路も確認しました。[記録](evaluation/model-budget-20261102.json)。今回の検証はメタデータと表示であり、GPUのピーク常駐量・性能・推論品質の再測定ではありません。次段階はバックエンドが報告するprefill/計算グラフ割当との照合と、MTP・履歴・メディアを含めた全体計画です。


## モデル準備時のディスク予算 — 2026-11-02

Rustの共通`DiskPlan`を準備画面、ローカルモデルの確認API、実行直前の容量検査に導入しました。必要なのは完成後のモデル容量ではなく、既存の準備見積もりに基づく「追加の空き容量」です。新規Metalダウンロードは`working_gb`、取得済みモデルは`working_gb - download_gb`を、GBから整数bytesへ変換して判定します。Q2_0はそれぞれ112,000,000,000／45,600,000,000 bytes、取得済みGemma 3 1Bの変換は2,500,000,000 bytesです。モデル別の既存見積もり自体や安全余裕は引き下げていません。

新規ダウンロードの対象は実行環境ディレクトリ、ローカル準備の対象は実行環境の親にある`Strata-data`です。実行環境がシンボリックリンクの場合は正規化後の親を使い、別のボリュームやリンク元の空き容量を誤って表示しないようにしました。モデル診断のMetal用ディスク表示も同じ正規化関数を使います。実モデルGGUFの配置先を調べる既存Gemma診断は維持します。

準備画面とライブラリーは書込先、必要な追加空き容量、不足量、空き容量未取得の案内を三言語で表示します。フルパスと見積もりの説明は情報アイコンにまとめています。容量不足なら準備操作を無効化し、再確認で解消したら有効に戻します。GUIで確認してから容量が変わった場合も、準備開始前にRustが再取得します。取得できない状態を0 bytesや容量不足と混同しません。従来と同じく、未取得だけを理由に実行を拒否はしませんが、再確認を促します。

APIは互換性のため`workingGb`／`freeBytes`等を残し、`diskPlans`（新規／ローカル）と`diskPlan`（選択モデル）を追加します。古い模擬データやサーバーに対しては従来表示へ戻ります。この予算はStrataのモデル準備用です。Hugging Face取得処理やアプリ同梱ファイルの初回設置の見積もりを置き換えるものではありません。

検証: `cargo test -p studio-core`は112件成功。整数容量の境界（必要容量ちょうど／1 byte不足）、容量0／未取得、新規・再利用の差、リンク経由の書込先を確認しました。`npm run build`と`node scripts/disk-budget-ui-smoke.mjs`も成功。Browser skillがないためPlaywrightを使用し、準備ダイアログを三言語・1440/390/320 pxで操作しました。表示と判定に異なる旧API空き容量999 GBを与え、共通計画の111 GBが優先されること、112 GB必要な状態から再確認して準備ボタンが復旧すること、同意後の準備コールバック、未取得への遷移、横はみ出しとコンソールを確認しています。容量サンプルと準備コールバックは模擬で、実ダウンロードや容量制限ディスクの再検証は今回行っていません。


## 同梱Gemmaの起動時割当報告 — 2026-11-02

モデル診断に、起動中の同梱Gemmaバックエンドが報告したKV・計算グラフ・出力バッファと、計算＋出力の合計を追加しました。既存の形状によるKV概算・作業用バッファ下限は残し、報告値で上書きしません。計算グラフの予約にはprefill/decode用の領域が含まれますが、これは全常駐メモリ・全作業領域・ピーク量の測定ではありません。モデル重み、履歴複製、未報告の一時割当などは合計に含みません。後から未報告の割当が変化しても、その量を推測で補いません。

固定llama.cppの標準ログでは概要が出ないため、明示的なCLIログ設定や`LLAMA_ARG_LOG_VERBOSITY`がない場合だけ`--verbosity 4`を追加します。モデル引数や設定ファイルは変更しません。明示的なログ無効化・別形式などで読めない場合は未取得を維持します。デバッグレベル5は有効にしません。既存ログ一覧の300件上限は維持します。

対象は、所有しているGemma子プロセスのstderrです。`llama_kv_cache`のKV報告、`llama_context`の出力報告、`sched_reserve`の計算グラフ報告を時刻付き／時刻なしで読みます。SWAと通常注意で同じデバイスのKV報告が複数ある場合は合計します。計算・出力はデバイスごとの最新値へ更新し、再割当を二重加算しません。デバイス数・KV報告数は各16件までで、異常な値・形式は未取得へ戻します。上流が0.01 MiB単位で丸めるため、表示は`≈`とし、bytesへ変換した数値も概数です。

収集先は起動ごとの独立したRustオブジェクトです。公開されるのは数値・セッションID・PIDだけで、トークンやGPUデータは送りません。停止時に切り離し、再起動では新しい収集先を作ります。表示前に現在の生存プロセス／セッションを再確認し、設定パス・実行環境・設定内容のSHA-256が起動時と異なる場合も非表示にします。古いstderrの読み取りが遅れて完了しても、新しいセッションに混入しません。保存状態や推論の受付・互換性検査は変更しません。

検証は`cargo test -p studio-core`の115件、`npm run build`、取得済みGemma 3 1Bによる一時的な共有Rustセッションの実起動2回です。KVは30 MiB、計算グラフは約55.54 MiB、出力1 MiB、計算＋出力は約56.54 MiBでした。静的なKV概算52 MiBと異なるのは、静的計画がSWAの節約を差し引かないためで、両者を速度向上や物理メモリ削減の証拠として比較しません。両回で認証付きAPIの`2+3`に`5`を確認し、停止後の報告非表示、再起動でのセッション変更、元設定ファイル不変も確認しました。通常の設定・会話は使用していません。最初の確認ではログレベル／報告形式の不一致で時間制限に達し、検証用プロセスを停止後、修正して上記の成功を得ています。

実報告JSONをGUI試験へ渡し、三言語・1440/390/320 pxの表示、停止状態への遷移、欠損値、コンソール、横はみ出しをPlaywrightで確認しました（Browser skillがないため既存Chromeを使用）。

```sh
# 既存モデルを実際に起動する任意の検証。通常の設定は読み込まず、終了時に停止します。
cargo run -p studio-core --example allocation-check -- /path/to/runtime /absolute/path/to/strata-gemma3-1b.json
SOLITON_MODEL_PLAN_FIXTURE=/path/to/model-plan.json node scripts/model-plan-ui-smoke.mjs
```

[検証記録](evaluation/gemma-allocation-report-20261102.json)。これは短い機能検証で、長時間性能・新モデル認定・別Macの確認ではありません。Strata Qwen、vLLM/MLX、Omniの動的割当・MTP・メディア・履歴の全量は引き続き次の作業です。依存更新等の別の作業中変更を保持した現作業ツリーで検証しています。


## vLLM-Appleの内訳を必要時に取得 — 2026-11-02

環境設定 → エンジン → モデル診断に、起動中vLLM-Appleの「実行中のメモリ診断を更新」を追加しました。認証済み`refreshBackendBudget`アクションが所有プロセスの`/v1/runtime`から取得し、アロケータ現在量／ピーク、論理KV、構成要素ごとの予算をRustで正規化します。`/health`は起動状態のみなので、詳細を含むと仮定しません。追加の定期ポーリングやGPU／モデル起動は行いません。

取得は全体2秒・64 KiB以内。成功結果は30秒まで共有し、画面に取得時刻を表示します。期限切れ・取得失敗では古い結果を表示せず、再更新が必要です。取得時刻は上流の計測時刻ではありません。バックエンドの監視処理は失敗時に以前の正常サンプルを保持するため、この表示を「今この瞬間の実測値」とは扱いません。非同期サンプルのアロケータ量から重みやKVを引いて作業量を逆算しません。

重み、再帰状態、プレフィックス、要求ウィンドウ、エキスパート、N-gram、MTP、scratch、Core MLを対象に、既知の出所を持つ項目だけを表示します。出所は「仕様予算」「要求見積もり」「混合台帳」「重みファイル量」に分類し、実測割当と区別します。`metal_heap`は重複領域を含むため加算せず、台帳側KVもテレメトリKVと重複表示しません。0が明示された場合は0、欠落・負数・不正形式・過大値・出所不明は未取得です。ピークが現在量より小さい等の不整合も推測で修正しません。これらを合計して全常駐量や適合判定を作りません。

取得の前後で選択エンジン・起動時設定・PID・ポート・私有キーを照合します。停止はキャッシュを消し、設定変更時も非表示になります。更新が重なった場合は新しい要求だけが結果を公開でき、以前のHTTP応答が遅れても上書きしません。既存の互換性・認定・入力数検査を維持し、私有キーや任意の上流文字列は診断データへ渡しません。

検証: `cargo test -p studio-core`は118件成功。数値／出所の検査、0と欠落の区別、重複を加算しない契約、ローカルruntime API取得、30秒経過、取得失敗時の消去、設定変更・停止時の非表示を確認しました。`npm run build`と`node scripts/backend-budget-ui-smoke.mjs`も成功。三言語・1440/390/320 pxで更新、未取得ピーク、仕様上のMTP=0、期限切れ相当の状態、再更新、停止時の無効化、横はみ出し・コンソールをPlaywrightで確認しました（Browser skillがないため既存Chromeを使用）。GUI値は模擬データです。

既存のGemma 2 / MLX設定・証拠を一時セッションへコピーした実起動も試しましたが、`architecture evidence identity mismatch`で起動ゲートが拒否しました。検証CLIは起動停止を早期検出し、後片付けしました。認定回避はせず、現在のMLX実モデルによる内訳取得は⭕️ [Pending]です。通常の設定・会話・モデルは変更していません。必要な場合は現在のバックエンド・環境に対する認定を更新してから、次を再実行します。

```sh
# VllmSettingsだけを含む一時JSONを指定。既存のモデルと認定が必要です。
cargo run -p studio-core --example allocation-check -- --vllm /path/to/vllm-settings.json
node scripts/backend-budget-ui-smoke.mjs
```

[検証記録](evaluation/backend-budget-diagnostics-20261102.json)。全GPU割当の測定や新しいモデル認定ではありません。別作業の依存更新等を保持した現作業ツリーで確認しています。


## 現環境のMLX再認定とネイティブGUI — 2026-11-03

前節のMLX実機確認保留は解消しました。旧証跡との差分は`runtime_sha256`のみで、モデル・バックエンド・環境・Macの識別値は一致。既存のGemma 2 2B 4-bit／MLX-LM 0.32.0を、コンテキスト4096、同時実行1、KV保持1024 MiB、アロケータ空きキャッシュ512 MiB、MPS fallback1のまま正式に再認定しました。検査省略、短縮試験、モデル取得、依存更新、認定対象ソースの変更は行っていません。1800.392秒、5038成功・失敗0、RSSピーク増分11.25 MiB、正常終了で新証跡が発行されました。[原本](evaluation/soliton-vllm-apple-requalification-20261103.json)。他アプリやビルドが動くデスクトップでの機能・安定性確認であり、過去の速度との比較ではありません。

実データで2点を修正しました。

- 現行の保持KVランチャーは`/vllm-apple/kv-cache`でスカラー値を返します。汎用のcontrol-runtime台帳だけではMLX使用量を取得できないため、Rustが起動時に確保した所有ワーカーの私有ポートから補完します。使用中・空きキャッシュ・使用中ピーク、保持LRUのbytes／件数を別々に保持します。後者を推論中の注意KVや物理RAMとして扱いません。既知の台帳形式・設定済み上限と一致するデータだけを採用し、取得不能なら今回の値は未取得になります。2応答合計64 KiB／全体2秒、セッション一致・30秒期限・停止時消去は維持します。
- 通常のGUI起動ではMPS環境変数が引き継がれないため、エンジン設定に`mpsFallback`を追加しました。未設定／nullは従来どおり継承、trueは`PYTORCH_ENABLE_MPS_FALLBACK=1`、falseは0を、診断と起動の両方へ渡します。三言語の選択欄と情報アイコンを用意し、起動中は編集できません。グローバル環境や他のプロファイルを変更せず、認定に一致する環境を再現します。値の変更で認定が自動的に有効になることはなく、不一致は拒否されます。

最新のネイティブSoliton.appを専用データ領域で起動し、外部のMPS環境変数を削除した状態から、保存された有効1だけで新証跡の「証跡が一致」・準備完了へ到達しました。GUIから「2+3の答えを数字1文字だけで返してください。」を送り、回答5を確認。診断ボタンから取得したチャット後の値は、MLX使用中1,528,095,300 bytes、空きキャッシュ10,126,522 bytes、使用中ピーク1,552,002,459 bytes、保持KV2,768,896 bytes／1件でした。画面は約1.42 GiB／9.66 MiB／1.45 GiB／2.64 MiBと表示。これらは非同時取得の別範囲で、合算や物理量への換算はしません。

英語・日本語・简体中文で実値の更新を確認し、30秒経過後の非表示、再更新、停止時の消去／更新無効、再起動で以前の報告を保持しないことも検証しました。新プロセスの保持KVは0 bytes／0件で、履歴の表示2件は残ります。UI操作はnative CUA、同じRust Stateの照合は認証付き読み取りAPIで実施し、通常の会話は検証に使いませんでした。狭幅は三言語のPlaywright fixtureで1440/390/320 pxを検証しています。

新証跡の正式な全内容照合に加え、旧証跡・4097コンテキスト・MPS0での拒否を確認してから、通常のnative/headless状態2件へ新証跡パスと明示的MPS1だけを適用しました。元ファイルのバックアップを作り、会話・他の全設定が一致することを確認。モデル、コンテキスト、キャッシュ予算、グローバル環境は保持しています。[GUI・適用記録](evaluation/mlx-native-memory-20261103.json)。アプリ・モデル・カウンターの検証用プロセスは停止済みです。

Rust120テスト、ビルド、配布前の71ネイティブファイル検査が成功。旧設定の継承、有効／無効の環境指定、実際の2エンドポイント、専用計測の取得失敗時に旧値を残さないことも検証しました。他Mac、別モデル、コンテキスト拡張、MTP・メディア全作業量、署名・公証は今回の承認範囲に含めません。


## SSD prefix cache prototype — 2026-11-04

Rustで状態ファイルを識別・検査して保存する試作と、実MLXキャッシュの保存・復元による出力一致の確認を追加しました。認証付きの検証用HTTPサーバーでは、再起動後の再利用、破損時の再計算、SSE切断後の復帰、容量超過時のフォールバックを実モデルで確認しています。通常アプリに独立した「MLX SSD」エンジンとして組み込み、起動・停止・切替・厳密な入力受付と容量枠設定を追加しました。専用の30分再資格試験は1800.302秒・2953成功・失敗0件、RSS増加約4.45 MiBで通過。対象はGemma 2 2B 4bit、入出力合計512、出力最大128、同時実行1の固定条件です。性能比較・重複書込み削減・他Macは引き続き次の作業です。[実装範囲・実測・再現手順](SSD_PREFIX_CACHE.md)を参照してください。


## SSD再保存の削減と比較 — 2026-11-04

保存済みprefixのファイル識別・更新情報を最大256件だけ記憶し、変更がなければ再シリアライズとRust保存を省略します。削除・破損・置換・権限変更は再保存の対象に戻し、保存失敗は記憶しません。実モデルでこれらと容量回復後の再試行を確認しました。

同一Mac・同一モデルをABBA順で比較した同一要求40件は、保存40→0回、最初のSSE内容までの中央値369.8→161.8ms、完了中央値554.8→344.9ms。交互の2要求40件は保存40→2回、全168回答は一致しました。コールド／再起動は各版2件だけで改善判定をせず、物理SSD書込みやRAM削減も主張しません。[条件・生データ・再現手順](SSD_PREFIX_CACHE.md#repeated-write-suppression-and-performance-comparison)。変更後の実行コードには新しい資格証跡が必要で、旧証跡の流用は拒否します。


## 複数ターン負荷とSSD容量管理 — 2026-11-04

環境設定のMLX SSDに容量パネルを追加しました。保存済み・一時・その他を分けて表示し、停止中に古い保存から容量枠の75%まで整理します。24時間以上前の識別可能なUUID v4一時ファイルも対象です。不明ファイル・新しい一時ファイルは保護し、稼働中は整理を拒否します。ワーカー外の作業領域と自動削除は今後の対象です。推論ワーカー・保存実行ファイルのハッシュは維持し、本体で既存の一致する資格証跡から起動を確認しています。

英語・日本語・简体中文の各6ターン、18履歴は空のキャッシュからの回答と全件一致。事実保持は9/9でしたが、厳密な出力形式は9/18で、句点や単位の追加が残りました。128MiBの圧迫試験では保存拒否6回でも推論は継続し、整理で122.21→83.37MiBへ減少。残した状態の復元、削除した状態の再計算・保存、次の保存省略も通過しました。入力23–155トークンの短い合成シナリオであり、長文や一般的なモデル品質の認定ではありません。[生データ・制限](SSD_PREFIX_CACHE.md#multi-turn-workloads-and-capacity-management)。


## エンジン性能の方針見直し（2026-11-08）

通常チャットの初回出力、継続生成、長い会話でのメモリを優先する。SSD比較の反復と周辺ツールの拡張を、エンジン開発の主目的から外す。これは優先順位の変更であり、バックエンドの優劣が新たに実測されたという意味ではない。既存の選択設定・資格証跡・実行コードは今回変更しない。

### 根拠と未確定事項

| 観察できていること | 方針への含意 |
| --- | --- |
| SSD再利用の旧ABBA試験は126/496入力で初回出力41.4%/57.7%短縮。ただし各モード4件、OSキャッシュを温めた直接HTTP計測 | 再起動後の再利用には見込みがある。通常会話・GPU計算・継続生成の高速化には一般化しない |
| 重複保存抑制は反復40件の保存40→0、初回出力369.8→161.8ms。メモリ減少は未立証 | 保存をしないことの効果は確認済み。保存対象・頻度・同期I/Oを優先して調べる |
| 現SSD実装は合計512トークン、RAM64MiB。SSD検索は入力末尾1トークンを除いた完全一致キー | 4,096トークンの主経路の代替にはまだならない。成長する会話の任意の過去prefixをSSDから再利用する機能は未実装。RAM側の近傍prefix検索と区別する |
| `prefix-cache-http.py` のprefill進捗callback内で同期シリアライズとRust CLIのputを行う。getもCLIとステージファイルを使う | 初回出力の経路に保存コストがあることはソースで確認。各コストの割合は未測定なので、全I/Oを即座に非同期化しない |
| 既存MLX主経路には常駐カウンター、保持KV/allocator制限、4,096境界試験がある | 既存経路を活用する。新しいランタイムへの全面移行より先に共通Rust APIで比較する |
| 24件の均衡比較は静穏条件を満たせず未完了。30分資格確認は安定性試験 | 性能と安定性を分ける。soak成功件数をtok/sやバックエンド比較に使わない |

根拠: [保存抑制](SSD_PREFIX_CACHE.md#repeated-write-suppression-and-performance-comparison)、[旧ABBA](evaluation/ssd-history-abba-comparison-20261107.json)、[モデル起動前拒否](evaluation/ssd-counterbalanced-preflight-20261108.json)、[既存主経路の採用試験](BENCHMARKS.md#512-mib-adoption--2026-10-28)。旧ABBAと現v2は実装identityが異なるため、v2の速度保証へ読み替えない。

### 実装する順序

| 優先 | 作業 | 完了条件 |
| --- | --- | --- |
| 🟠 [Next] 1 | Rust共通APIの待ち時間分解。起動・証跡照合・入力計数・上流待機・最初の内容・完了を関連付け、backendが取得できるprefill/decode/cache load/save/CLI時間を別spanとして追加 | 同一request ID/engine sessionで対応。未取得は未取得、失敗/中断も残す。既存計測を重複実装せず不足spanを補う。非同期・CPU/GPUの重複時間を単純加算しない |
| 🟠 [Next] 2 | 常駐MLXを中心に、RAM prefix再利用と会話継続を評価。モデルを毎ターン再ロードしない。保存頻度・短いprefixの永続化の採算を調べる | 新規入力・同一prefix・成長履歴・会話切替・再起動を分離。RAM hit/SSD hit/missを区別し、再利用トークンと実使用量を保持 |
| 🟠 [Next] 3 | 計測で支配的だったコストを一つ削減。SSDは保存の選別・要求完了後の安全なsnapshot、CLI常駐化等から最小変更を選ぶ | GPU stateを参照したままバックグラウンドserializeしない。snapshot追加メモリ、停止/キャンセル、次要求との競合を上限付きで検証。利益が小さければ同期経路を維持 |
| 🔴 [Later] 4 | 長文prefillの分割、KV保持/量子化、長prefixのSSD復元を個別評価 | メモリと速度、EN/JA/ZH品質の交換条件を測定。4,096へのSSD拡大は形式/容量/境界レビューと別資格確認が先 |
| 🔴 [Later] 5 | decodeが支配的な場合に投機的デコードや別backendを比較 | 同じモデル/量子化/入力/出力条件、draft追加メモリと受理率を含む。既存主経路を上回る証拠が出てから統合 |
| ⭕️ [Pending] | 静穏時の24件SSD均衡比較、実再起動/別Mac検証 | 保留証跡は維持。静穏条件待ちの自動再試行を通常の開発作業にしない |

主経路候補は既存の`vllmApple`設定で使う`mlx_lm`とする。この設定名はvLLM本体と同一の実行方式を意味しない。Strataは大きいモデルの既存SSDストリーミング用途、Omniはメディア用途、MLX SSDは永続prefixの実験用途として別々の適性を評価する。既定エンジンの自動変更、別エンジンの削除、現時点でのMLX最速宣言は行わない。RustはDocument/State・所有権・入出力・計測を保持し、モデル計算には既存のネイティブMetal backendを利用する。推論kernelsのRust/wgpu全面再実装やWebViewへのstate転送は優先しない。

### 比較・採用ルール

通常操作の基準を、起動から最初の回答、常駐後の新規入力、成長する3言語履歴、会話切替、長い回答のdecode、停止/復旧に分ける。モデル・重み量子化・template・実入力数・出力上限・samplingを固定し、実出力数/終了理由も保存する。出力が短かっただけの速さを採用理由にしない。異なる512/4,096コンテキストや異なるmodelの速度は同列に比較しない。現SSDとの共通範囲試験と、主経路だけの長文試験を分離する。

最初は段階別spanと中央値で大きな待ちを見つける。少数標本からp95保証や有意差を出さず、採用候補が絞れた後に独立した複数blockとtail latency/peak physical footprintを評価する。品質・正確な入場判定・失敗/中断・停止時回収は必須とする。探索用の暫定採用目安は対象シナリオTTFT中央値10%以上改善、他の主要シナリオ中央値5%超の悪化なし、既定メモリ枠内とするが、これは未達の計画値であり性能保証ではない。品質やメモリに交換条件がある候補は自動採用せず明記する。

低負荷比較と通常デスクトップ負荷での使用感試験は別cohortにする。既存の静穏ゲートは緩めず、通常負荷試験の数字で不合格の静穏比較を埋めない。通常負荷でも機能/エラー/キャンセル検証とspan収集は進められるが、因果的な速度改善は主張しない。写真解析等のOSサービスは自動停止しない。

探索は隔離されたcandidateと短い正確性/性能試験で進める。効果のある変更をまとめたrelease candidateに対し、元の30分資格確認・完全identity照合・共通API境界/ライフサイクル試験を通してから運用設定を切り替える。探索モードや文書更新を証跡更新の代わりにしない。証跡の期限や内容検査を省略した起動最適化は採用しない。

上流では[prompt cache、prefill分割とKV容量の交換条件](https://github.com/ml-explore/mlx-lm/blob/main/README.md#long-prompts-and-generations)、[投機的デコードとdraftの方式](https://github.com/ggml-org/llama.cpp/blob/master/docs/speculative.md)が文書化されている（2026-11-08参照）。これらは候補技術の根拠であり、ローカルの固定版・モデルでの動作や速度を証明するものではない。

English: Prioritize end-to-end first-content latency, resident RAM-prefix reuse and measured hot-path costs. Keep SSD persistence secondary; do not repeatedly block development on quiet-host benchmarks. Preserve exact admission, memory bounds and full release qualification. No backend migration or runtime setting change was made by this review.

简体中文：优先改善公共API的首内容延迟、常驻RAM前缀复用和已测出的关键路径。SSD持久化降为次要方向，不再让静穏负载测试反复阻塞开发。保留精确准入、内存上限和完整发布资格验证。本次只调整方针，未迁移后端或修改运行设置。


## Gateway段階別計測（2026-11-08）

性能方針P0の最初の実装として、既存`Metrics`に省略可能な`gateway`を追加した。`/studio/state`の直近100件の`requests`、チャットの保存済み`inference.metrics`から取得できる。GUI表示項目は今回追加していない。従来の`elapsedMs`/`firstOutputMs`/`firstAnswerMs`の開始境界は維持し、新しい受付基準の値と混ぜない。旧保存データでは`gateway`は未取得となる。

| gatewayフィールド | 意味 |
| --- | --- |
| `engineSession` | 起動ごとに発行する公開UUID。同じState直下の`engineSession`と対応し、停止中はState側がnull。worker認証キーとは独立 |
| `lifecycleWaitMs` | ライフサイクルmutexの取得待ち時間 |
| `admissionMs` | validation、入力計数とその待機を含む、入場判定開始からdispatch準備完了までのwall time。純粋なtokenizer CPU時間ではない |
| `admissionRejected` | 明示的な上流送信前拒否はtrue、送信に進んだ要求はfalse。未判定はnull。既存statusはerror/completed/cancelledを維持 |
| `dispatchMs` | handler受付から上流送信開始までのoffset |
| `upstreamHeadersMs` | 上流送信開始からHTTPヘッダー取得までのduration。失敗/中断で未取得ならnull |
| `firstContentMs` | handler受付から最初の空でない回答・推論・tool内容をRustが観測するまでのoffset。role通知は含めない |
| `elapsedMs` | 同じ受付時計による記録完了/失敗/中断までのwall time |

小さい待ちも観測できるよう小数ミリ秒を保存する。値はGPU実行時間でも互いに排他的な内訳でもない。`dispatchMs`には入場判定等が含まれるため、各列を合計しない。受付はRust handlerの開始で、チャットではAxum JSON抽出の後となる。ネットワーク送信開始からの完全なクライアント時間ではない。JSONの内容はbody収集・parse時に観測するため、`firstContentMs`はモデルの最初のtoken時刻ではない。SSEとJSONを一つのTTFT集計に混ぜない。

認証済みAPIの明示拒否を履歴に残し、チャットではbackend入力計数による拒否も記録する。拒否された会話本文は変更しない。認証失敗、チャットのJSON抽出/ユーザー入力不備/オフライン/会話競合など計測開始前の拒否は今回の記録対象外。未到達のdispatch/header/contentはnullのまま保持する。上流エラーとストリーム中断には既存の記録終了処理を使い、rateやprefill時間を推定しない。

160 Rust tests、server/Tauri buildが通過。追加テストは旧データ互換・独立時計・未到達値・拒否/中断に加え、20msのlifecycle待機、20msの入力計数、10msの上流ヘッダー待機を伴うAPI/チャットの統合経路を確認した。これは実エンジンの性能値ではない。

専用QA状態と既存v2証跡で実Gemma2 2Bを起動し、API JSON/SSE・チャットSSE・129-token出力予約の拒否を確認。3件完了/1件拒否、セッション一致・再起動時更新・停止時消去、拒否前後の会話一致が通過した。通常デスクトップ負荷下の機能確認であり、速度比較ではない。資格確認済みworker/helper/store executableのハッシュは変わらず、通常設定は変更していない。QAサーバーと所有するモデルは停止済み。[元の計測値と検証範囲](evaluation/gateway-spans-20261108.json)。

次は主経路MLX-LMの4,096構成で、常駐後の新規入力・成長履歴・会話切替の基準値を取得する。証跡照合を含む起動全体、backend内部のcache load/save/CLI/prefill/decodeの不足span、WebViewへの表示は未実装/未検証として分けて扱う。

English: Optional gateway wall-time spans now cover lifecycle wait, admission, dispatch, upstream headers, first content and completion, correlated with a public engine session. Old timing fields retain their origin. JSON content observation is not model TTFT; GPU/cache-internal costs and the primary 4096-token baseline remain unfinished.

简体中文：新增可选的网关阶段计时及公开引擎会话ID，涵盖生命周期锁等待、准入、发送、上游响应头、首次内容与完成。原计时字段保持原定义。JSON内容观测时间不等于模型首token时间；GPU/缓存内部耗时及主路径4096 token基线仍待完成。


## 4,096トークン主経路の基準値と0.1.1（2026-11-08）

既存の主経路を起動すると、Homebrew側の開発版MLX-LMと外部RECORDリンクを完全inventory検査が拒否した。検査を緩めず、グローバル環境を変更せず、34パッケージの固定環境を専用フォルダーへ構成した。MLX-LM0.32.0、MLX0.32.4、Transformers5.20.2、Python3.12.16。既存KV wrapperの旧source pairを保ち、レビューしたPyPIのserver/cache組だけを追加許可した。実LRUの論理byte上限試験と混在/改変拒否を確認し、完全fingerprintも通過。再現用[依存pin](../scripts/fixtures/mlx-0320-runtime-20261108.txt)、[適用パッチ](../patches/vllm-apple-mlx-0320-pypi.patch)、[元の起動拒否](evaluation/mlx4096-runtime-diagnostic-20261108.json)を保存した。

新環境はcontext4,096/concurrency1/retained KV1,024MiB/allocator512MiB/MPS fallback1で、元の30分検証を完走した。**1800.116秒、12,744成功、失敗0、正常終了、RSS peak growth442,368 bytes**。三言語品質、greedy/stream整合、開始/終了identity一致を含む。成功件数やsoakのrequest/sは主経路の速度比較に使用しない。[完全資格証跡の公開記録](evaluation/mlx4096-fixed-runtime-qualification-20261108.json)。

その証跡で専用Rustサーバーを起動し、変更前/後それぞれ36件をSSE経由で測定した。内訳は短文の新規/反復6件、履歴長を変える12件、前のメッセージを保持して追記する9件、異なる長履歴のAPI切替6件、長めの生成3件。履歴長だけを変える試験は既存のturn区切りも変わるため、真の追記と別phaseにした。API切替はGUIライブラリー切替の検証ではない。さらに各runでEN/JA/ZHの4,096ちょうどを3件受理、4,097を3件拒否。fixture matrix digest、全36件の入力数・cached数、33件の厳密なcode回答、3件の自由文出力が一致した。最初の27件pilotは探索記録として別に残す。[変更前](evaluation/mlx4096-gateway-before-20261108.json)、[変更後](evaluation/mlx4096-gateway-after-20261108.json)、[要約](evaluation/mlx4096-baseline-summary-20261108.json)、[pilot](evaluation/mlx4096-gateway-pilot-20261108.json)。

| 主な観測 | 変更前 | 変更後 |
| --- | ---: | ---: |
| 起動→readyの観測時間 | 3.806s | 3.794s |
| 最初の要求の入場判定 | 204.26ms | 4.27ms |
| 短文反復のfirst content中央値（各3件） | 56.99ms | 53.23ms |
| 長履歴切替のfull hit（各2件）の中央値 | 105.39ms | 99.16ms |
| 長履歴切替のmiss/partial（各4件）の中央値 | 11.13s | 8.55s |
| sampled engine physical footprint最大 | 3,933,719,456 bytes | 3,930,606,472 bytes |

これは通常デスクトップ負荷での**基準値・機能回帰確認**である。1 runずつの順次測定で、CPU/GPU/温度を制御した比較ではない。上流の秒単位の差、反復の数ms差、3MB程度のengine footprint差を計数変更の効果とは断定しない。warm cacheとmissでは処理する入力が違い、上表も一律の高速化率にはしない。engine footprintは要求後の標本で、真のpeakや別groupのCPUカウンターRSSとは異なる。`timings`未取得ならdecode GPU速度を推定しない。初回計数の多くは既にmodel loadと重なっており、CPU単体の短縮がそのまま起動短縮になるわけではない。

### 採用した改善と次の優先順位

バージョン不一致でAutoTokenizerへfallbackしていた軽量計数を、測定した**Transformers5.20.2/tokenizers0.23.3/Jinja2 3.1.6の組**でも使えるようにした。従来5.19.0の組は維持する。4つのtokenizerファイルの内容ハッシュ、未知sidecar/versionのfallback、要求前後の変更検出を維持した。1,091入力（raw211/chat844/今回の会話36）で完全なtoken-IDが一致し、不正role3件と6つのfallback guardも通過した。[ID一致](evaluation/mlx4096-token-parity-20261108.json)。

独立CPU workerの5組の交互試行では初回計数中央値**3235.01→1120.00ms（65.38%減）**、計数後RSS中央値**315.72→201.84MiB（36.07%減）**、warm計数中央値12.35→12.18ms。全計数は同一。この試験中は資格確認のGPU処理が動作しており、背景負荷を完全制御した因果推定ではない。各afterの初回時間とRSSはそれぞれbeforeを下回ったが、cold CPU初期化とRSSの限定された結果として扱う。[全10試行・source hash](evaluation/mlx4096-token-worker-20261108.json)。Rust160 tests、baseline5 tests、既存Node27 tests、Python証跡/volume/comparator/source guard/protocol試験、frontend/server/Tauri buildも通過。

最も大きい待ちは、長履歴が保持枠から外れた場合の再計算にあると推測できる。次はKV1GiBをcontrolに、追加保持の候補1.5/2GiBとprefillの観測を比較する。OSの余裕、sampled/peak physical、キャッシュmiss数、長文TTFT、短文/生成の回帰を評価し、利益があるcandidateだけを別途完全資格確認する。現時点で保持枠やallocator、contextを増やさない。SSD比較の再試行や新backend追加より、既存RAM再利用の改善を優先する。

通常アプリの設定は変更していない。専用のusable証跡は`.soliton-runtime/engine4096-20261108/qualification.json`で、その専用Python/launcher/model/runtime/Macに束縛される。旧Homebrew設定は現在も拒否されるため、通常起動を切り替える際は公開記録ではなく対応するprivate証跡と固定ランタイムを明示選択する。QAホストと所有モデルは停止済み。署名・公証、別MacはPending。0.1.1はソースリリースとして区切り、以後も検証完了した節目にpatchを0.0.1増やす。

English: A fixed private MLX runtime completed the original4096-token30-minute qualification. Thirty-six shared-API cases plus three exact-boundary admissions/refusals passed before and after the tokenizer selector update, with identical answers and counts. CPU-worker first count/RSS improved in five paired observations; no controlled whole-engine speedup is claimed. Long-history recomputation is the next priority. Ordinary preferences are unchanged.

简体中文：固定的独立MLX环境完成原有4096 token、30分钟资格验证。修改计数器前后，各36个公共API用例及三语言边界受理/拒绝均通过，答案与计数一致。五组CPU计数器观测显示首次计数及RSS下降，但不宣称整机推理加速。下一步优先减少长历史重算；普通设置未改动。
