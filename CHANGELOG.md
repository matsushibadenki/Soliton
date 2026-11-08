# Changelog

## 0.1.1 — 2026-11-08

- Added correlated Rust gateway wall times and per-start engine session IDs, preserving legacy timing fields.
- Added durable volume-bound v2 qualification for opt-in MLX SSD; legacy certificates remain refused.
- Added cache comparison preflight, balanced ABBA/BAAB ordering and explicit rejected-run evidence.
- Added a fixed, isolated PyPI MLX-LM0.32.0 runtime snapshot and reviewed source-pair patch for the 4,096-token path.
- Extended the measured lightweight tokenizer selector to Transformers5.20.2 while preserving5.19.0; full token-ID parity and shared-API regression passed.
- Added a multilingual shared-API baseline tool. Real results and qualification scope are linked from `docs/ROADMAP.md`.

This is a source release. Signed/notarized native installers and other-Mac validation remain pending. No ordinary model/settings selection is changed by installing the source tree.

日本語：検証が完了した区切りでパッチ版を0.0.1ずつ更新します。速度比較・機能検証・資格証跡の範囲を分けて記録します。

简体中文：每完成一个验证里程碑，将补丁版本增加0.0.1；速度比较、功能验证与资格记录分别注明适用范围。
