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

同梱するのは Strata のソースです。Python 3.12.15と実行時依存ライブラリを同梱します。ビルド済みMetalエンジンはまだ含まれません。既存の準備機能は対応する Python とネットワークを必要とします。Hugging Face のリポジトリURL、tree/リビジョンURL、個別ファイルURLを指定できます。リポジトリは全ファイルを取得するため、空き容量を確認してください。任意のモデルが推論可能になることを保証しません。トークンはメモリ内だけで使用し、処理完了時に入力欄を消去します。

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

再配置後の依存ライブラリimportは開発Macで検証できます。Metalエンジンの初回ビルドは依然としてAppleのCommand Line Toolsに依存するため、開発環境のないMacでの推論は未達です。Pythonランタイムとwheelに含まれるライセンスはアーカイブ内に保持します。
