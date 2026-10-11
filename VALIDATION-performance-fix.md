# 0.1 performance fix — validation, 2026-10-11

The app name is **Muguet**. The release and launcher version label is **0.1 performance fix**; the numeric executable version is 0.1, build 7, with the original bundle identifier and iOS 15 minimum.

All runtime, graphics, controller, memory, import-cleanup and HUD source files match R5 at `218381d8fd46143b6a0b4cb16a7e53dde2c070c8` byte for byte. This includes the complete vendored wgpu tree. Changes are limited to release identity, documentation and build-output paths. The launcher differs from R5 only in its version label. MetalFX remains absent; the 5 GiB heap policy and GPU timing default of Off remain unchanged.

The existing controller tests and native selected-ZIP cleanup/protection tests were rerun and passed. R5's prior seven app tests, 67 wgpu-core tests, 1,113 vertex-shader checks, 32 pixel/lifetime results and five-repeat synthetic tracking measurements are retained in the original validation evidence; they were not rerun for this identity-only restoration. See [VALIDATION-fork-r5.md](VALIDATION-fork-r5.md) for their scope.

The restored arm64 iOS release build and Xcode package completed with two build jobs. ZIP CRC, staged executable equality, original Muguet app identity, lack of signatures/resource seals/provisioning profiles, presence of the R5 HUD and absence of MetalFX links/exports were checked. The embedded game module is identical to R5: 213,444,632 bytes, SHA-256 `faff37a8b07d05e44abd82f6caa1ed2caed78951df788e5b4ce85d69865f0271`. The original game Wasm and data ZIP were not changed. All 547 existing license/notice files were retained.

Personal unsigned IPA: **92,509,265 bytes**, SHA-256 `d34d10e131bbd379675959935d91dc29c267acd6110409061d6d194156d4505b`. Signing and installation remain the owner's manual iLoader workflow. The public GPL release contains source/documentation only; no game-bearing IPA, Wasm or game assets are uploaded.

The later before/after owner captures show 59.9 to 30.2 FPS at the same selected/presented 2150×990 resolution, with native renderer CPU increasing from 10.9 to 32.0 ms/frame and Metal encode from 4.5 to 15.3 ms/frame. Draws change from 3,042 to 3,258, pending draws remain near zero, and GPU timing is off. These captures support increased CPU-side cost but do not establish its cause. This release restores R5 as requested; it does not claim to resolve that observed transition or guarantee 60 FPS. The newly labeled package has not been signed, installed or tested on an iPhone.
