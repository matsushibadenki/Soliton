# Build cache cleanup and model re-preparation · 2026-11-02

## English

🟢 [Done] Removed only reproducible Rust debug artifacts: `target/debug/incremental`, `deps`, `.fingerprint`, and `build`. No compiler/model processes were using them at the initial check. Finished apps, release artifacts, standalone debug/test executables, all existing `Strata-data`, `.soliton-runtime`, ordinary state and legacy app-data directories were retained. Target size fell from about 14 GiB to 4.7 GiB. Immediate filesystem free-space change was 9,637,933,056 bytes (about 9.64 GB); the cache cleanup measurement recorded 43,730,894,848 → 53,368,827,904 free bytes. The volume continued changing independently during validation; later total free-space increases are not all attributed to this cleanup.

🟢 [Done] Before cleanup, recorded 123,513 existing protected files/symlinks across models, runtime stores and conversation/app-data state. After cleanup and after the complete validation, no recorded existing file was removed or changed: regular-file inode/size/mtime, symlink targets, and SHA-256 of JSON/state files up to 16 MiB matched. This is not a new full checksum of every large weight file. Existing model weights and conversation/settings content were not written by the validation.

🟢 [Done] With capacity genuinely sufficient, `test:reuse` passed existing-config activation, missing-shard refusal, successful supervised fixture preparation without model downloads, three languages and four responsive widths. No capacity guard was disabled or mocked.

🟢 [Done] Full-size real Q2_0 re-preparation also completed through the authenticated common Rust API. Extracted the already-built Strata 0.2.9 distribution into a dedicated temporary workspace, selected the existing two Q2_0 GGUF files, and required `inspectLocalModel` to return `kind=prepare` / `diskPlan.status=sufficient`. Called `useLocalModel`; the real upstream converter regenerated the dense pack/tokenizer and about 34 GB SSD expert layout. Upstream `--verify` confirmed 48 layers × 512 experts identical to the GGUF. The resulting job reported completed, the engine remained offline until explicitly started, and the generated config was applied only to isolated state. Existing production configs, history and model stores were retained.

🟢 [Done] The newly prepared model reached ready with 8192 context, then returned exact synthetic code REPREP112 through authenticated `/v1/chat/completions`: 24 input / 7 output tokens, prompt 33033.5 ms and decode 3014.4 ms. This is a functional test, not a performance benchmark. Raw API inference did not alter the temporary common Document. Stop returned offline; the owned gateway shut down with exit code 0. The harness initially waited for a nonexistent running state; continuation used the actual ready state to verify the already-prepared model without repeating preparation. This was a harness correction, not a model failure.

🟢 [Done] After successful inference and shutdown, removed only the new temporary validation workspace (35,597,239,680 bytes of regular files, including the newly generated expert layout). Original files linked into it were retained. Final preservation check again matched all 123,513 recorded files. Measured free space after temporary cleanup was 111,571,824,640 bytes (about 104 GiB); a later df snapshot showed about 108 GiB due to ongoing volume changes. No unrelated models or data were deleted.

[Cleanup evidence](evaluation/build-cache-cleanup-20261102.json) · [Real preparation, generated config, logs, inference and final preservation evidence](evaluation/model-reprepare-20261102.json).

The prior disk-capacity Pending item is resolved. Native another-Mac validation, Developer ID/notarization, unrelated model families and longer performance qualification are not established by this test. Future Rust debug builds will regenerate the removed caches.

## 日本語

🟢 [Done] 再生成可能なRustデバッグ用キャッシュ4ディレクトリだけを削除し、直後の計測で約9.64 GBを解放しました。完成済みアプリ・リリース成果物・検証用実行ファイル、既存モデルと会話データは保持しました。

🟢 [Done] 保護対象123,513ファイルを事前・事後で照合し、既存ファイルの削除・記録属性変更はゼロでした。JSON/状態ファイルはSHA-256も照合しています。巨大モデル全体を新たにハッシュしたという意味ではありません。

🟢 [Done] 空き容量ガードを通したUI再利用テストと、既存Q2_0の実際の再準備が成功しました。一時領域で約34 GBのSSD派生データを生成し、48層×512エキスパートの元GGUFとの一致を確認。新しい設定で起動し、認証付き実推論のREPREP112完全一致、正常停止、会話状態維持を確認しました。

🟢 [Done] 成功後、今回の検証専用複製約35.6 GBだけを削除して空き容量を戻しました。最終照合でも既存モデル・会話に変更はありません。通常設定は切り替えていません。保留だった容量不足による再準備検証は解除します。

## 简体中文

🟢 [Done] 仅删除四个可重新生成的Rust调试缓存目录，即时测得释放约9.64 GB。保留现有模型、会话、应用、发布产物及测试可执行文件。

🟢 [Done] 对123,513个已有保护文件进行前后比对，没有删除或记录属性变化；JSON/状态文件另核对SHA-256。这不代表重新计算所有大型模型的完整哈希。

🟢 [Done] 在真实容量检查通过后，UI复用测试及现有Q2_0的真实重新准备成功。临时生成约34 GB SSD派生数据，上游验证48层×512专家与GGUF一致。使用新配置启动后，认证推理精确返回REPREP112，正常停止且公共会话状态不变。

🟢 [Done] 成功后仅清理本次新建的约35.6 GB验证副本，恢复空间；最后比对仍无已有数据变化。未切换普通配置。此前因容量不足而待定的重新准备验证已完成。
