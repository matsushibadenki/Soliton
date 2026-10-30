# Clean Mac validation / 別Mac検証 / 独立Mac验证

⭕️ [Pending] Another Mac has not been tested. 別Macの実機確認は保留中です。尚未在另一台Mac上实测。

The local preflight checks archive integrity, engine checksums, arm64 binaries, deployment targets, dynamic dependencies and relocated Python imports. It does not prove compatibility with another Mac or older macOS. Validation builds use ad hoc signatures for bundle integrity. Developer ID signing and notarization remain deferred. This is a validation build, not a public release.

現環境の配布検査は、アーカイブ、エンジンハッシュ、arm64、最低OS、動的依存、再配置したPythonのimportを検査します。別Macや古いmacOSの動作を証明するものではありません。検証用のad hoc署名でアプリ全体の整合性を検査します。Developer ID署名・公証は保留中です。このセットは検証用です。

本机分发检查覆盖归档、引擎校验值、arm64、最低系统版本、动态依赖及Python迁移后的导入，不代表其他Mac或旧版macOS的兼容性验证。验证包使用ad hoc签名检查完整性，Developer ID签名与公证仍待完成，本包仅用于验证。

## Build host / ビルド用Mac / 构建Mac

```sh
npm run desktop:build -- --bundles app
npm run check:distribution -- --smoke
npm run package:validation
```

`desktop:build` automatically runs the static preflight afterward. `--smoke` additionally extracts Python to a temporary directory and tests imports with system-only PATH. `package:validation` reruns that check, creates a ZIP, verifies it and writes SHA-256 plus a pending-status manifest to `.soliton-runtime/distribution-handoff/`. The report records the repository HEAD and whether the worktree is dirty; use an isolated clean checkout for a reproducible release. No model weights or credentials are added.

`desktop:build`の後に静的検査を自動実行します。`--smoke`では一時ディレクトリへPythonを展開し、PATHを標準コマンドだけにしてimportを確認します。`package:validation`は再検査・ZIP作成・ZIP検査・SHA-256と保留状態の記録を行います。保存先は`.soliton-runtime/distribution-handoff/`です。HEADと未コミット変更の有無も記録します。再現可能な正式配布にはクリーンなチェックアウトを使用してください。重み・認証情報は追加しません。

`desktop:build`完成后自动静态检查。`--smoke`将Python解压到临时目录，在仅包含系统命令的PATH下验证导入。`package:validation`重新检查、生成并验证ZIP，保存SHA-256和待验证状态至`.soliton-runtime/distribution-handoff/`。记录HEAD及未提交更改状态；正式发行需使用干净的检出目录。不包含权重或凭据。

## Target Mac / 対象Mac / 目标Mac

Apple Silicon, macOS 15 or later. No Xcode, Homebrew, system Python, Node or Rust is required on the target. Record available memory and storage; allow runtime installation space plus the model source and at least 2.5 GB free for conversion.

Apple Silicon・macOS 15以降が対象です。対象Macへの開発ツールの追加は不要です。メモリ・空き容量を記録し、実行環境の設置容量、元モデル、変換用2.5 GB以上の空きを確保します。

目标为Apple Silicon及macOS 15以上，无需安装开发工具。记录内存及可用空间，预留运行环境、源模型和至少2.5 GB的转换空间。

| Step | English | 日本語 | 简体中文 |
| --- | --- | --- | --- |
| 1 | Record chip, macOS, RAM, storage and developer-tool availability. | チップ・OS・メモリ・空き容量・開発ツールの有無を記録。 | 记录芯片、系统、内存、空间和开发工具安装情况。 |
| 2 | Verify ZIP SHA-256, extract and open the app. Record the OS launch result. | ZIPのSHA-256を照合して展開・起動。OSの起動結果を記録。 | 校验ZIP、解压并启动，记录系统启动结果。 |
| 3 | Use a fresh test account or isolated app data; preserve existing user files. | 検証用アカウントか分離した保存先を使用し、既存データを保持。 | 使用测试账户或独立数据目录，保留原有数据。 |
| 4 | Confirm first-run Python and dependencies are ready. | 初回設置でPythonと依存ライブラリの確認済み表示を確認。 | 确认首次安装完成，Python及依赖已就绪。 |
| 5 | Add the existing full Gemma folder in Local models; select model.safetensors and Use in Strata. | ローカルモデルで元フォルダーを追加し、重みを選んで「Strataで使用」。 | 添加完整Gemma目录，选择权重并点击“在Strata中使用”。 |
| 6 | Confirm 0 GB model download, 2.5 GB workspace and preparation complete. | モデル追加取得0 GB、作業領域2.5 GB、準備完了を確認。 | 确认无需追加下载模型、工作空间2.5 GB、准备完成。 |
| 7 | Start Strata; use 64 output tokens and temperature 0. | Strataを起動し、最大トークン64・Temperature 0で会話。 | 启动Strata，输出上限64、Temperature 0。 |
| 8 | Test the three languages and a follow-up; confirm completion. | 三言語と追質問を送り、応答と完了表示を確認。 | 测试三种语言和追问，确认响应完成。 |
| 9 | Quit and reopen, start again without conversion and confirm chat/history. | 終了・再起動し、再変換なしの起動・チャット・履歴保持を確認。 | 退出重启，无需再次转换即可聊天，历史记录保留。 |

Gemma source folder: `config.json`, `model.safetensors`, `tokenizer.model`, `tokenizer_config.json`; retain the other JSON files in the original repository too. Revision: `dcc83ea841ab6100d6b47a070329e1ba4cf78752`. Weight SHA-256: `3d4ef8d71c14db7e448a09ebe891cfb6bf32c57a9b44499ae0d1c098e48516b6`.

- 日本語: `日本の首都はどこですか？都市名だけで答えてください。` → `東京`
- English: `What is 2+2? Reply with only the digit.` → `4`
- Follow-up: `What number did you just answer? Reply with only that digit.` → `4`
- 简体中文: `中国的首都是哪里？只回答城市名。` → `北京`

Record actual responses even if they differ. 記録は実際の回答をそのまま残してください。请如实记录实际回答。

## Result record / 結果記録 / 结果记录

Record the ZIP SHA-256, chip/model (no serial number), macOS, RAM, storage, stage, actual result, error text and whether a retry recovered. Separate **not run**, **passed** and **failed**. Keep tokens, passwords, session API keys and private conversation content out of reports. A launch blocked by the OS is a launch-stage result, not an inference result. Do not disable OS protections to make the test pass.

ZIPハッシュ・機種／チップ（シリアル番号不要）・OS・メモリ・空き容量・工程・実結果・エラー・再試行結果を記録します。未実施／成功／失敗を分け、トークン・パスワード・セッションAPIキー・私的な会話を含めないでください。OSの起動制限は起動工程の結果として記録し、推論結果とは分けます。検証のために保護機能を無効化しないでください。

记录ZIP校验值、机型／芯片（无需序列号）、系统、内存、空间、步骤、实际结果、错误和重试结果。区分未执行／通过／失败，不记录令牌、密码、会话密钥或私人对话。系统阻止启动应记录为启动阶段结果，不作为推理结果；不要为通过测试而关闭系统保护。
