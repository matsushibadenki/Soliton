# Gemma 4 31B ARA-heretic: Mac preflight

2026-11-10 · Static assessment; inference and qualification pending.

## Conclusion / 結論 / 结论

**EN:** Apple M4 with 32GiB has a plausible native llama.cpp/Metal path, but this exact GGUF has not run and Soliton does not yet admit Gemma4. Use Q3_K_M as the conservative first memory smoke candidate. IQ4_XS and Q4 variants require measured headroom; Q5 and larger are poor initial choices on this Mac. Quantization quality is untested.

**日本語:** このM4・32GiBではllama.cpp/Metalが実行経路の候補です。対象GGUFの実行は未確認で、Solitonへの組み込みも未対応です。初回のメモリ検証はQ3_K_Mを候補とし、IQ4_XS・Q4系は実測で余裕を確認して判断します。Q5以上は初回候補から外します。量子化による品質差は未評価です。

**简体中文:** M4、32GiB可考虑原生llama.cpp/Metal路径，但尚未运行该GGUF，Soliton也尚未接入Gemma4。首次内存验证优先考虑Q3_K_M；IQ4_XS和Q4系列须先测量余量。Q5及以上不适合作为本机的首次候选，量化质量差异尚未评估。

## Observations

- Hardware: Apple M4, 34,359,738,368 bytes (32GiB), macOS27.2 (26B123), approximately328GiB disk available.
- Existing native binary reports Apple M4 via Metal, budget25,559MiB (~24.96GiB). This is a backend budget, **not additional RAM or actual free system memory**. No model was loaded.
- Runtime pin: `3cf03257f219afbe7334045ff7c6a06ac68c627d`. Gemma4 loader and Metal kernel code are present; source support and device discovery do not certify exact-model execution.
- Target revision: `1b49bbdcb3dfaa67cc4daced7da7d16552959f97`. Only the first16MiB of Q4_K_M was downloaded in bounded HTTP ranges. The parser read metadata and833 tensor descriptors, not tensor contents. Whole-model hashes are not verified.
- Header: `gemma4`,60 layers (50 sliding /10 full), hidden5376, SWA1024, advertised context262144. Local KV:16 heads ×256; global KV:4 heads ×512. Advertised context is not a qualified context or memory-fit claim.
- Current Soliton budget accepts Gemma2/Gemma3 HF configs and Gemma3 GGUF; its Gemma3 preparation adapter also rejects this architecture. Preserve those guards and add a separate reviewed profile.

## Memory planning (text only)

GiB uses2³⁰ bytes. File size is a storage proxy, not measured resident memory. One sequence,4096 context, microbatch128, F16 K/V:

| Quant | GGUF file GiB | Illustrative total GiB, compact SWA |
|---|---:|---:|
| Q3_K_M | 14.24 | 25.5–27.8 |
| IQ4_XS | 15.70 | 27.0–29.2 |
| Q4_K_S | 16.54 | 27.8–30.1 |
| Q4_K_M | 17.40 | 28.7–30.9 |
| Q5_K_M | 20.35 | 31.6–33.9 |
| Q6_K | 23.47 | 34.8–37.0 |
| Q8_0 | 30.39 | 41.7–43.9 |

Totals = file size + estimated KV + **assumed**3–4GiB working/loader allowance + **assumed**7GiB OS/app reserve +0–1.08GiB possible tied-embedding duplication. These allowances are planning choices, not measured peaks or guaranteed upper bounds. Other quant headers have not been parsed; their totals provisionally assume the inspected architecture. mmap, placement, graph buffers and active applications change actual residency.

At4096 context, compact SWA uses1280 cells: KV≈1.289GiB with FlashAttention,1.445GiB without it. Full-size SWA uses4096 cells: KV≈3.438–3.594GiB, adding about2.15GiB to the table. Thus Q4_K_M is conditional, not a reliable32GiB fit claim. With microbatch512, compact KV instead becomes1.484–1.641GiB. Multiple sequences, larger contexts or vision need separate budgets.

Derivation: local bytes =50 ×4096 ×2(K,V) ×2(F16) ×SWA_cells; global bytes =10 ×(2048 K +2048 V with FA, otherwise4096 V) ×2 ×context. SWA_cells = ceil256(min(context,1024+microbatch)) for this single-sequence case, or context with full-size SWA. K=V projection sharing does **not** eliminate the allocated V cache. Cache allocator overhead is excluded.

The Q4 header has tied embeddings and no separate output tensor. The loader can reuse a tensor in the same buffer context; placement into different contexts may duplicate its~1.08GiB Q6_K embedding. This is an allowance, not an observed duplication. mmproj files are separately0.75GiB(Q8_0)/1.12GiB(f16), excluding their execution buffers; vision is outside this assessment.

## Next gate / 次の工程 / 下一步

1. Add a dedicated Gemma4 metadata/budget profile and supervised native-runtime adapter; validate exact tokenizer/chat-template counting and admission. Do not route through Gemma3 or reuse its qualification.
2. Pin a selected quant and verify the complete checksum, then run isolated text smoke tests at4096 context, one sequence, microbatch128. Record actual cache mode, FA availability, load/peak memory, swap, prefill/decode speed and recovery. Stop on failed admission or memory pressure; no automatic global runtime replacement.
3. Only after smoke success, test EN/JA/简体中文, multi-turn/cache/cancellation/restart and exact identity, then run a separate full30-minute qualification. Vision and other quants remain separate Pending gates.

日本語：次は専用アダプターとメモリ判定を用意し、選択した量子化で実測します。実行・速度・ピークメモリ・30分の安定性は未確認です。

简体中文：下一步添加专用适配器与内存准入，再验证所选量化的实际运行。速度、峰值内存和完整30分钟稳定性尚未确认。

## Evidence and primary sources

- [Machine-readable assessment](evaluation/gemma4-31b-mac-preflight-20261110.json): sizes, declared LFS hashes, header metadata, prefix checksum and reproducible KV arithmetic.
- [Pinned GGUF repository](https://huggingface.co/mradermacher/gemma-4-31B-it-ARA-heretic-GGUF/tree/1b49bbdcb3dfaa67cc4daced7da7d16552959f97).
- [Pinned Gemma4 loader](https://github.com/ggml-org/llama.cpp/blob/3cf03257f219afbe7334045ff7c6a06ac68c627d/src/models/gemma4.cpp).
- [Pinned SWA allocation](https://github.com/ggml-org/llama.cpp/blob/3cf03257f219afbe7334045ff7c6a06ac68c627d/src/llama-kv-cache-iswa.cpp) and [KV allocation](https://github.com/ggml-org/llama.cpp/blob/3cf03257f219afbe7334045ff7c6a06ac68c627d/src/llama-kv-cache.cpp).
- [Pinned tensor placement/reuse](https://github.com/ggml-org/llama.cpp/blob/3cf03257f219afbe7334045ff7c6a06ac68c627d/src/llama-model-loader.cpp).
