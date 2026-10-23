# Upstream compatibility · 2026-10-21

Studio's `Strata` gitlink now pins `9de04cd44e927a9a3df6bbc0e94fc1be8b844ba4` from [Niko1221/Strata](https://github.com/Niko1221/Strata/commit/9de04cd44e927a9a3df6bbc0e94fc1be8b844ba4). This is the Strata 0.2.1 line, with engine 0.1.46 and the Control-only source-version patch 0.1.46.3. The local Metal build's `BUILD.json` reports 0.1.46.3. The previous gitlink was `f85fbb151bd11542d686ca1f35ee22b7bc5e3f05`; that local commit remains on its existing branch. Its tracked Metal headers were retained as a local build dependency. Upstream tracked sources were not patched.

`.gitmodules` records the upstream URL so a fresh clone can initialize the pinned checkout:

```sh
git submodule update --init Strata
npm run test:upstream
cargo test -p studio-core
```

## Integration changes

- Mac setup now imports planning from `strata_control.mac_fit`. Studio disables optional MTP fetching both in the setup module and in the shared planner. This includes resident/large-memory profiles; disabling only the old alias could leave `--mtp` in the generated arguments without preparing those weights.
- `data_folder_for` passes four migration/default-folder arguments. Local reuse accepts both old and new calls and returns only its owned workspace. It never migrates another app's files or saves a new upstream default folder.
- Runtime and build dependencies are now separate upstream. The official builder supplies build tools when necessary. Studio retains the dedicated `.venv` and invokes the source builder; existing model files are reused.
- New `serve.lifeline` starts the native engine in its own process group. Mac memory sampling now follows the owned server's child/descendant relationships, including the native engine and its lifecycle watcher, instead of assuming they share a group. Unrelated process trees are excluded. Existing metadata fields remain compatible. Shared allocations may still be charged more than once; this is a sum of physical footprints, not unique pages.
- Studio's authenticated loopback launch and existing chat API remain in use. The new upstream desktop/Terminal installer is not launched. Image/vision preparation remains explicitly off in Studio's current setup flow.

## Validation and limits

- The real new Mac setup module passes the offline local-reuse contract check for 32/128 GiB, with no MTP arguments, no model downloads and no data migration. The wrapper remains compatible with legacy setup calls.
- 56 Studio Rust tests pass, including the four-argument preparation contract and a native child started in a separate process group. Upstream Mac setup's 24 tests pass.
- Both the Metal engine and Studio's Apple Silicon app build successfully. Existing Q2_0 shards and their prepared pack are reused. No model download occurred.
- Upstream `serve.test_server` and `serve.test_responses`: 384 tests ran; 382 passed, one skipped, one VisionArgs fixture errored on macOS. Its fake `Proc` has no `pid`, but the new lifeline watcher expects one. This is an upstream test-fixture limitation, not a successful vision test; upstream sources were left unchanged. Studio keeps vision disabled. GPU backends other than Metal and additional model families were not validated here.

For future upstream updates, run the offline contract first, rebuild Metal from the new source rather than keeping an old executable, then exercise local-file preparation and authenticated real inference. Preserve the selected memory settings, conversations and stopped/running state. Do not infer compatibility with new model families solely from upstream support.

Final real inference on Apple M4 / 32 GiB: the existing Q2_0 returned the exact synthetic verification code, 35 input / 8 output tokens, in 10.864 seconds. Twelve memory samples each counted three processes (server, native engine and watcher); sampled footprint peaked at 7.63 GiB. These short-request samples are not a performance benchmark of the release. [Raw measurement](benchmarks/metal-upstream-20261021.json). The native Engine tab showed the summed memory, then “—” after stopping. The six-message document, conversation metadata, layout and Studio settings matched their pre-test snapshot. The 8K context, prompt chunk 64 and 7 GB reserve were restored after upstream re-preparation by Studio’s existing policy.
