# macOS 配布手順（開発段階）

## 現状

- 🟢 [Done] ビルド時に固定された Strata サブモジュールのソースをアプリへ同梱し、初回起動時にユーザーデータ領域へ展開する。
- 🟢 [Done] モデルライブラリから Hugging Face の公開ファイル URL を指定して取得できる。対応形式は選択時に既存の検査で確認する。
- 🟠 [Next] Python と依存ライブラリの配布・初回セットアップを含む、別の Mac での単独動作を検証する。
- 🟢 [Done] 公開ファイルの取得中に進捗を表示し、設定画面から GitHub Releases の更新を確認できる。
- 🟢 [Done] リポジトリの全ファイル取得、固定コミットに対する HTTP Range 再開、任意の読み取りトークンによる認証ヘッダー送信。
- ⭕️ [Pending] 有効なアクセス権が必要な gated/private モデルでの実機認証確認。
- 🟠 [Next] 署名された更新パッケージの検証とアプリ内適用。
- 🔴 [Later] Developer ID による署名、公証、DMG 配布と GitHub Releases の自動化。
- ⭕️ [Pending] 実モデルのダウンロード、Metal エンジンのビルド、推論をクリーンな実機で確認する。

同梱するのは Strata のソースです。Python 3.12.15と実行時依存ライブラリを同梱します。ビルド済みMetalエンジンと必要なシェーダー資源も同梱します。既存の準備機能は対応する Python とネットワークを必要とします。Hugging Face のリポジトリURL、tree/リビジョンURL、個別ファイルURLを指定できます。リポジトリは全ファイルを取得するため、空き容量を確認してください。任意のモデルが推論可能になることを保証しません。トークンはメモリ内だけで使用し、処理完了時に入力欄を消去します。

更新確認はリリースページへ案内する機能です。自動インストールはまだ行いません。GitHub の非公開リポジトリや API 制限によって確認できない場合があります。Tauri updater の配布には Apple のコード署名とは別の更新署名鍵と公開鍵の設定が必要です。

## ビルド

配布物を作成するMacには Apple Silicon、Xcode Command Line Tools、Rust、Node.js、npm、uv が必要です。サブモジュールの指定リビジョンを取得してからビルドします。

```sh
git clone --recurse-submodules <repository-url>
cd Soliton
git submodule update --init --recursive
npm ci
npm run desktop:build -- --bundles app
```

`bundle:engine` は親リポジトリが固定する Strata のコミットからアーカイブを作成します。ローカルの未コミット変更は同梱されません。生成された `.app` は `target/aarch64-apple-darwin/release/bundle/macos/` にあります。未署名のため、配布先の Gatekeeper で警告や起動制限が生じます。

## 検証

```sh
npm run build
cargo test --manifest-path src-tauri/Cargo.toml bundled_engine_tests
cargo check --manifest-path src-tauri/Cargo.toml
```

配布物をクリーンなユーザー環境で起動し、初回展開、再起動後の既存ファイル保持、設定画面のモデル選択、公開ファイル URL の取得とライブラリへの表示、エンジン準備と実推論を確認します。大容量モデルのダウンロード前に空き容量を確認してください。既存データを消さず、検証用ユーザーを使用してください。

## GitHub への反映

変更内容と未コミット差分を確認し、今回の変更だけをステージします。既存の別作業は混ぜません。

```sh
git status --short
git diff
git add <今回変更したファイル>
git diff --cached
git commit -m "Bundle Strata source and add model download"
git push origin <branch>
```

GitHubへのプッシュはソースコードの公開です。バイナリ配布が必要な場合は、ビルドと実機検証を完了し、署名・公証を実施した後に GitHub Release に添付します。Apple の認証情報をコミット、ログ、Release に含めないでください。

## モデル取得と再開

停止や通信エラーでは隠し一時ファイルを保持します。同じURLを再送信すると、固定コミットの途中バイトから取得を再開します。サーバーがRangeを無視した場合は最初から取得します。mainが更新された場合は新しいコミットの別フォルダに取得します。完了ファイルは再取得しません。修復が必要なファイルは削除してから取得してください。

読み取りトークンは設定画面のパスワード欄に入力します。gatedモデルは事前にHugging Faceで利用条件に同意し、アクセス権を取得してください。トークンは保存されないため、再試行時は再入力します。

2026-10-29: `sshleifer/tiny-gpt2` の実取得で、個別ファイル取得、再取得、100バイトの途中ファイルからの再開と元ファイル一致、リポジトリ全取得を確認しました。実推論とgated/privateモデルは未検証です。

## 同梱Python

ビルド時にuvで取得した再配置可能なCPython 3.12.15へStrataのrequirements.txtをインストールし、python.tar.gzを作成します。初回起動時にユーザーデータ領域へ展開し、依存ライブラリのimportに成功した場合だけ設置を完了します。新規設定ではこのPythonを選択します。既存設定のPython指定は維持されます。

再配置後の依存ライブラリimportは開発Macで検証できます。配布用コピーはビルド済みエンジンを使用します。開発環境のない別Macでの実推論確認は保留です。Pythonランタイムとwheelに含まれるライセンスはアーカイブ内に保持します。

## ビルド済みMetalエンジン

`bundle:metal` は親リポジトリが固定するソースを専用作業ディレクトリに展開してコンパイルします。配布アーカイブにはエンジン、SHA-256、gguf-pyとライセンスを含めます。配布用コピーの準備処理は、コンパイラ確認を省き、ハッシュを確認したエンジンを使います。新規環境ではStrata-metal-v1へ展開します。既存のカスタムエンジン設定は自動移行しません。

2026-10-29: 別ディレクトリへの展開後、同梱Pythonで依存ライブラリ読み込み、コンパイラなしとした環境情報での検査、エンジンSHA-256検査を確認しました。実モデルをロードする推論と開発ツールのない別Macでの確認は保留です。

## 実モデルの配布資源検証

` .soliton-runtime/python-relocation-test/bin/python3 -I scripts/verify-bundled-inference.py` は、この開発環境にあるQ2_0モデルを使う明示的なGPU検証です。配布アーカイブを新規展開し、同梱シェーダーと共有ヘッダーへ参照を固定し、PATHを`/usr/bin:/bin`に限定します。元のモデルや設定は変更しません。

2026-10-29: 入力トークン`1,2,3`に対して出力`4`を生成し、終了コード0を確認しました。この検証で共有ヘッダーの取り違えを発見し、Strataのビルドで使用する`third_party/ggml/ggml-common.h`へ修正しました。これはエンジンの1トークン生成テストであり、文章品質、GUI経由の初回準備、別Macでの動作は引き続き保留です。

## GUI通し検証（2026-10-29）

🟢 [Done] Apple Siliconの開発Macで、配布アプリを空の専用データ領域から起動し、GUI操作で以下を確認しました。

1. 同梱Python・同梱Strataの初期設定を確認。
2. ローカルモデル画面から既存のQ2_0シャードの保存先を追加し、「Strataで使用」を実行。
3. トークナイザーとパック、SSD配置データを新規生成し、「モデルの準備が完了し、設定を反映しました」を確認。モデル本体の再ダウンロードはなし。
4. GUIの「起動」から「準備完了」へ遷移。
5. チャットで `Reply with exactly OK. Do not explain.` を送信し、応答 `OK` と生成完了を確認。表示指標は23.75秒・23トークン・5.5 tok/s（性能保証用のベンチマークではありません）。
6. アプリの終了操作で検証用エンジンも終了。

この検証で、ローカルモデル再利用用の仮想環境が同梱ライブラリを参照できずnumpyのimportに失敗する不具合を修正しました。同梱エンジンの印がある場合だけ仮想環境へ同梱Pythonのsite-packagesを公開し、既存の失敗した環境も再試行で修復します。通常の外部Pythonを指定した環境は従来の分離設定を保持します。

再現時は、絶対パスの `SOLITON_DATA_DIR` を指定して配布実行ファイルを起動すると、エンジン展開先と保存設定を検証用ディレクトリに分離できます。検証では `.soliton-runtime/gui-first-run` を使用しました。Native GUIはComputer Useで操作・画面確認しました。Browser plugin skillは利用できず、今回の対象がネイティブGUIだったためPlaywrightの代わりにネイティブ操作で通しました。

🟢 [Done] 新規モデルのURL取得からGUIチャットまでの検証（下記参照）。

⭕️ [Pending] 開発ツールをインストールしていない別Macでの確認、長時間の会話品質と性能の検証。

## 初回ナビゲーションと自動復旧

- 🟢 [Done] モデル未設定のデスクトップ主ウインドウでは、初回接続後にモデル準備画面を自動表示します。プラットフォーム・Python・依存ライブラリを確認し、既存モデルの選択または対応モデルの準備へ進めます。モデルのダウンロードは利用者の操作と既存の同意手順に従います。
- 🟢 [Done] 起動時に同梱Pythonの依存ライブラリを再検査し、破損・不完全な設置を退避して再展開します。不完全なエンジン設置も同様に復旧します。
- 🟢 [Done] 退避先は元ディレクトリ名に`.recovery-UUID`を付けた同じ親ディレクトリです。設定などを回収できるよう、退避データを自動削除しません。
- 🟢 [Done] 空き容量不足・アクセス権など設置失敗時のGUI内修復案内と再試行（下記参照）。
- ⭕️ [Pending] 別Macでの検証。

自動復旧テストでは、設定ファイルを含む不完全なエンジン設置を作成し、再設置と退避先の元設定保持を確認しています。

2026-10-30: 空の`gui-guided-first-run`領域で配布アプリを起動し、準備画面の自動表示、Python 3.12.15と依存ライブラリの確認済み表示、容量表示、ダウンロード同意前の準備ボタン無効化をネイティブGUIで確認しました。

## 設置失敗時のGUI修復（2026-10-30）

- 🟢 [Done] 初回設置の失敗を保持したままGUIを起動し、容量不足・書き込み権限・その他の設置失敗に応じた案内を表示します。
- 🟢 [Done] 設置前に書き込み確認と空き容量検査（実行環境用2 GiB、モデル用容量は別）を行います。
- 🟢 [Done] 日本語・英語・简体中文で保存先、対処方法、詳細と再試行を表示します。再試行はRustの排他制御下で実行し、成功後は画面を再読み込みして初回設定へ戻ります。
- 🟢 [Done] 実機GUIで、検証用保存先の権限を500にして書き込み失敗を発生させ、案内を確認。700へ戻した後、GUIの再試行で設置が成功し、Pythonと依存ライブラリが確認済みとなる初回準備画面への復帰を確認しました。
- 🟢 [Done] 容量不足の判定、OSエラー分類、読み取り専用領域の拒否をRustテストで確認。模擬エラーを使ったブラウザーテストで容量不足・権限エラー、再試行失敗、3言語を確認しました。
- 🟢 [Done] 容量制限したテストディスクを実際に満杯にしてGUIから復旧する検証（下記参照）。
- ⭕️ [Pending] 別Macでの確認。

再現用UIテスト: `node scripts/installation-repair-smoke.mjs`。Browser skillが利用できないため、既存PlaywrightとChromeを使用します。必要に応じて`PLAYWRIGHT_CHROMIUM_EXECUTABLE`を指定します。テスト用HTMLと画像は`.soliton-runtime`内に置き、HTMLは正常終了時に削除します。

## 容量制限ディスクでの実機GUI検証（2026-10-30）

🟢 [Done] 4 GiB指定のHFS+ sparse disk image（実ボリューム約3.7 GiB）を作成し、アプリの`SOLITON_DATA_DIR`をその中に指定しました。ホストの実ディスクではなく、この検証専用ボリュームにのみ埋め草ファイルを書き込みました。

1. `scripts/fill-installation-test-disk.py`で埋め、実際の`ENOSPC`（errno 28）を確認。書き込み済みカウントは3,925,868,544 bytes、`df`の使用率は100%。
2. 配布アプリを起動すると「設置用の空き容量が不足しています」、保存先と2 GiB確保の案内が表示されました。
3. 空き容量を増やさず「再試行」しても修復画面が維持され、再試行ボタンが利用可能なことを確認。
4. このテストで作成した`soliton-disposable-capacity-fill.bin`だけを削除し、約3.7 GiBの空きを確保。
5. GUIの「再試行」で設置に成功し、初回モデル準備画面へ自動復帰。同梱Python 3.12.15と依存パッケージが確認済みになりました。
6. 設置後のディスク使用量は約370 MiB、空きは約3.3 GiB。モデル準備画面はモデルの作業領域不足を別途表示し、準備ボタンを無効化しました。
7. アプリを終了し、テストボリュームをアンマウントしました。製品コードの追加修正は不要でした。

再現手順（macOS、テスト専用）:

```sh
mkdir -p .soliton-runtime/space-disk-test/mount
hdiutil create -size 4g -type SPARSE -fs HFS+ -volname SolitonSpaceTest .soliton-runtime/space-disk-test/test.sparseimage
hdiutil attach .soliton-runtime/space-disk-test/test.sparseimage -mountpoint "$PWD/.soliton-runtime/space-disk-test/mount" -nobrowse
python3 scripts/fill-installation-test-disk.py .soliton-runtime/space-disk-test/mount
SOLITON_DATA_DIR="$PWD/.soliton-runtime/space-disk-test/mount/data" target/aarch64-apple-darwin/release/bundle/macos/Soliton.app/Contents/MacOS/soliton
```

GUIの案内確認後、別ターミナルからテストボリューム内の上記埋め草ファイルだけを削除して、GUIで再試行します。終了後は`hdiutil detach "$PWD/.soliton-runtime/space-disk-test/mount"`でアンマウントします。同名の既存テストイメージがある場合は、新しく作り直さずその内容を確認してください。充填スクリプトはマウントポイント以外・ホストと同じデバイス・5 GiB以上のボリュームを拒否し、既存の埋め草ファイルを上書きしません。

## Hugging Face URLから新規取得してGUIチャットまで（2026-10-30）

🟢 [Done] 空の`.soliton-runtime/gui-hub-new`領域で配布アプリを起動し、ネイティブGUIから以下の流れを確認しました。既存モデルのコピー・リンクをダウンロードの代わりに使っていません。

1. 初回準備画面からモデルライブラリへ進み、Hugging Faceの個別ファイルURLを入力して2シャードを新規ダウンロードしました。
2. リポジトリは`ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-GGUF`、リビジョンは`ed59f92082b1e93c0e96d60a8b11aab089b52f09`、ファイルは`Q2_0/Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00001-of-00002.gguf`と`00002-of-00002.gguf`です。入力URLは`https://huggingface.co/<repo>/resolve/<revision>/<file>`形式です。
3. 取得量は37,623,740,192 bytesと28,800,138,432 bytes（合計66,423,878,624 bytes）。両ファイルが通常ファイルであること、サイズと固定SHA-256の一致を確認しました。
4. GUIでダウンロード済みモデルを選び「Strataで使用」。新規のパック・トークナイザー・SSD配置データを生成し、設定反映完了を確認しました。準備時にモデルを再取得していません。
5. 最終修正版の配布アプリに切り替えて再起動し、設定保持と同梱エンジンの起動を確認。GUIから`Reply with exactly OK. Do not explain.`を送信して`OK`の応答と完了表示を確認しました。
6. 実行設定の`--native`と`--ple-gguf`の実体が、この新規取得先を参照することを確認しました。表示値は28.18秒・23トークン・3.9 tok/sです（性能保証のためのベンチマークではありません）。
7. 検証後はアプリと推論エンジンを終了しました。新規モデルと準備データは検証領域に保持しています。通常の利用者設定は変更していません。

検証記録: `docs/evaluation/huggingface-gui-20261030.json`。ハッシュ再検証: `python3 scripts/verify-hub-q2-download.py <ダウンロード先のQ2_0ディレクトリ>`。`--shard 1`または`--shard 2`で片方だけを照合できます。

この検証で以下を修正しました。

- ダウンロード先をアプリの実行環境保存先と統一し、`SOLITON_DATA_DIR`を指定した場合も検証領域へ保存。
- 大容量取得の進捗通知を約250 msごとに制限。進捗行と百分率、停止操作を整理し、説明を情報アイコンへ移動。
- Q2_0の初回準備で必須の約34 GBのSSD配置データを容量見積もりへ含め、総作業領域を112.0 GB（既存ファイル利用時45.6 GB）へ修正。最終版GUIで112.0 GB表示を確認。

進捗UIは`node scripts/hub-download-ui-smoke.mjs`で、模擬IPCを用いて幅720 pxの画面、50%表示、停止操作の表示、アプリ例外がないことを確認できます。これは実ネットワーク取得の代替ではなく、表示の回帰テストです。

⭕️ [Pending] 別Mac、gated/privateモデルの有効な認証、長時間の会話品質・性能の確認。

## 認証必須モデルの実機GUI検証（2026-10-30、Gemma 3対応前の記録）

- 🟢 [Done] `google/gemma-3-1b-it` の固定リビジョン`dcc83ea841ab6100d6b47a070329e1ba4cf78752`について、認証なしの重みRange GETが401で拒否されることを確認。
- 🟢 [Done] 利用者がGUIのパスワード欄へ直接入力したトークンで、リポジトリのダウンロードが完了。エージェントはトークンを読み出していません。完了後の入力欄消去と保存設定に認証用フィールドがないことを確認。
- 🟢 [Done] 重み1,999,811,208 bytesのサイズと公開メタデータのSHA-256（`3d4ef8d71c14db7e448a09ebe891cfb6bf32c57a9b44499ae0d1c098e48516b6`）が一致。
- 🟢 [Done] 検証用GUIでMLX LMのローカルモデル・実行環境を指定し、診断と起動を試行。現行vLLM-Appleが`gemma3_text`を`architecture_unknown`として拒否することを確認。
- 🟢 [Done] この拒否をGUIの単なる「未検証」として扱う表示を修正。未対応アーキテクチャとしてblockedを返し、情報アイコンに3言語の対処案内を表示。
- 🟢 [Done] 後続のGemma 3対応・資格確認・GUIチャット検証が完了しました。下記の追加検証を参照してください。
- ⭕️ [Pending] 非公開リポジトリ、別Macでの確認。

記録は`docs/evaluation/huggingface-auth-gui-20261030.json`です。**この時点では認証付きダウンロードまで成功し、推論は当時のエンジンが拒否しました。後続の成功結果は下記に分けて記録しています。**互換性検査のスキップや未検証アーキテクチャの強制起動は行っていません。vLLM-Appleは別リポジトリの外部エンジンであり、この検証では開発Macの既存MLX環境を診断しています。

最初に指定されたGemma 4のURLはこの環境では認証なしでも重み取得が可能だったため、認証必須性の成功証拠には含めていません。


## Gemma 3対応とGUIチャットの追加検証（2026-10-30）

- 🟢 [Done] 外部[vLLM-Appleの33043c01](https://github.com/matsushibadenki/vLLM-apple/commit/33043c01a379f885c8adc5b9a83b02415c16b60c)で`gemma3_text`を認識。周期的なローカル／グローバル注意、独立したRoPE設定とhead dimensionなどを保持し、不正な周期・矛盾するレイヤー定義を拒否します。
- 🟢 [Done] 取得済みの上記固定リビジョンをMLX-LM 0.32.0で正式に資格確認。英語・日本語・简体中文の品質検査、30分の連続推論6,770件すべて成功、エラー0、終了処理成功。
- 🟢 [Done] 最終配布ビルドのGUIでモデルフォルダーと証跡を指定して診断し、「証跡が一致」→起動→準備完了を確認。日本語・英語の算数質問にどちらも`2`を返しました。
- 🟢 [Done] 入力と出力上限が検証済みコンテキストを超える場合、3言語で環境設定 → APIへの修復案内を表示。GUIで上限512による拒否を再現し、64へ下げて応答まで復旧を確認しました。
- ⭕️ [Pending] 別Mac、より長いコンテキスト、Gemmaのマルチモーダル構成、同梱StrataでのGemma 3推論。

### 再現手順

外部vLLM-Appleの対応ソースと互換Python環境、MLX-LM 0.32.0を準備します。モデルはGUIのローカルモデル画面から取得済みフォルダーを選びます。この経路は同梱Strataとは別の実行環境です。

資格確認コマンド（パスは各Macの実体へ置換）:

```sh
/path/to/python -m vllm_apple qualify-model /path/to/gemma-3-1b-it \
  --backend-executable /path/to/mlx_lm.server --backend-kind mlx_lm \
  --backend-port 18091 --max-model-len 512 --duration 1800 --warmup 30 \
  --concurrency 1 --phase-samples 3 --phase-output-tokens 16 \
  --mode text --bind-architecture-evidence --candidate-mlx-lm-version 0.32.0 \
  --output /private/path/gemma3-qualification.json
```

vLLM-Appleのルートを作業ディレクトリにして実行してください。候補バージョン指定はインストール済みバージョンとの一致を検査し、互換性検査を省略しません。完了した証跡は所有者限定で保存し、GitHubへ公開しないでください。

環境設定 → エンジンでvLLM-Apple／MLX LM、モデル・Python・バックエンド・証跡の絶対パス、コンテキスト512を指定し、診断後に起動します。APIタブの最大トークンは今回64、Temperatureは0。会話が長くなると512の範囲を超えるため、新しい会話へ切り替えるか同じ環境でより大きなコンテキストを再検証してください。

証跡はモデル、バックエンド、ラッパーのソース、Macと環境、有効期限に結び付きます。別Macやソース・モデル・バックエンド更新後は再生成が必要です。今回の結果で全Gemmaモデルや任意バージョンを包括的に許可していません。公開可能な検証要約は`docs/evaluation/gemma3-gui-20261030.json`です。
