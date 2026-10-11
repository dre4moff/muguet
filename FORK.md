# Muguet — 0.1 performance fix

This fork starts from c22dev/Muguet commit `63737647eb3f7fee2e2139995606e3b02a5920ee`. The original author, copyright notices, GPL-3.0-or-later license, and vendored MIT/Apache licenses are retained. This is an independent modification by dre4moff, not an official Rockstar or upstream release.

## Release identity

The app is named **Muguet**. **0.1 performance fix** is the release/version label shown in the launcher, not the app name. The R5 implementation at `218381d8fd46143b6a0b4cb16a7e53dde2c070c8` is retained, including its controller fixes, successful-import ZIP cleanup and opt-in performance HUD. Build 9 changes the archive-selection/access flow and adds direct import from the app's own Documents folder; renderer, controller, memory and embedded game code are unchanged. No new performance experiment is included. The launcher shows the build number to distinguish installed revisions. See [VALIDATION-import-build9.md](VALIDATION-import-build9.md).

## Performance investigation

r5 targets the CPU limit captured in the owner's new HUD screenshots: 26.5–27.5 native renderer CPU ms per 27–28 ms frame, about 98% of one core. It removes repeated shader-description copies/string cache keys and reuses empty command tracking tables after their original resource lifetime ends. Distinct resources remain owned and validated. The HUD separates deferred Metal encoding from queue submission, shows pass/submit/pending-draw counts, and makes GPU timestamp profiling a separate off-by-default diagnostic option. Pixel/lifetime and destroyed-resource checks validate host correctness; no 60 FPS or measured iPhone gain is claimed. See PERFORMANCE.md and VALIDATION-fork-r5.md.

r4 adopts upstream's Binaryen `wasm-opt -O3` preprocessing before iOS native compilation (patching hooks/exports first). It also removes a per-draw uniform allocation and redundant full-block copy while preserving zero padding and all rendering inputs. Its original opt-in HUD added native renderer/process CPU time, command-ring waits, existing GPU fence waits, drawable acquisition, submission/uniform costs, readback/group counts, I/O service cost and bounded asynchronous GPU pass timestamps. See [PERFORMANCE.md](PERFORMANCE.md) for exact definitions and limitations. No iPhone FPS gain is claimed by host checks. Controller and 5 GiB heap policy remain unchanged.

## Original rendering restored

The experimental MetalFX integration from r1/r2 has been completely removed after the owner reported reduced clarity and FPS. r2 kept the selected input dimensions but multiplied reconstruction dimensions, increasing output pixels by 2.25× or 4×. It also added an image-motion search, full-frame conversion, history copying and cross-queue synchronization. It lacked engine-provided geometry motion vectors; its low-cost synthetic correctness tests did not establish quality or performance for the game on iPhone.

The source establishes extra GPU work and unreliable motion reconstruction. It does not establish that the numerical game input resolution actually fell; the reported appearance can also result from incorrect temporal reconstruction. No reliable device-validated repair is available with the current data, so r3 returns to the upstream vertex, pipeline, draw and presentation code. r4 retains the original rendering inputs and adds the bounded optimization and diagnostics described above. The extra scaler, shader jitter, motion/history buffers, native exports, framework link, launch selector and configuration handling are gone. Old saved scaler keys are removed when the launcher saves settings; they cannot enable any rendering code in r3. Resolution and texture settings remain available.

## Launcher options

- **Performance HUD**: disabled by default. Displays visible presentation FPS, average presentation interval (not GPU execution time), process memory footprint, remaining iOS process memory budget, approximate total process budget, current linear heap / ceiling, render/output sizes, draw count, graphics validation errors, controller connection, and thermal state. It updates twice per second and does not intercept touch input.
- **Physical controller**: separate look sensitivity (0.25–4, default 1.5) and radial deadzone (5–30%, default 12%). The right-stick response is independent of touch and mouse sensitivity. Apple Game Controller's extended gamepad profile is used; individual hardware has not been certified. r3 retains r1 controller code: the owner confirmed the controller is connected and works correctly, so no new controller changes were made.

## Controller/app issues and game limitations

| Symptom | Cause / change in this fork | Acceptance still required |
| --- | --- | --- |
| Slow camera with a physical controller | App shared mouse gain and used a steep per-axis curve. Separate gain and a radial response now control the right stick. | Camera, aiming and wheel selection on an iPhone. |
| Radio wheel would not open | App only sent Left Arrow. Holding D-pad Left in a vehicle now also holds Q, the radio-wheel key; phone arrows remain available. | Hold, select with right stick, release; check game content supports stations. |
| Character wheel would not open | App only sent Down Arrow. Holding D-pad Down now also holds Left Alt; phone arrows remain available. | Verify in Story Mode at a point where multiple protagonists are available. Sandbox, missions and game scripts may prevent character switching. |
| Sprint/reload while navigating pause | App also sent gameplay keys. The game script reports actual pause state; paused A/B use confirm/back without sprint/reload, and both sticks/D-pad navigation are separated from gameplay. | Pause navigation and transitions. Phone/mission Enter/Backspace behavior is retained. |
| Stuck input after disconnect/backgrounding | App retained controller state and horn. Controller keys, mouse buttons and horn are now released on focus loss and disconnect. | Disconnect/reconnect and app background/resume. |
| Steering feels digital | Upstream emulates keyboard/mouse; left-stick movement and triggers still use digital key thresholds. | This fork does not claim native analog steering or universal remapping. |

## ZIP import and storage

Build 8 still stalled on the owner's iPhone when confirming **Open**. Build 9 presents `UIDocumentPickerViewController` directly from the launcher hosting controller using UIKit `present`, rather than embedding the picker in a SwiftUI cover. The selection callback starts import directly; it no longer waits for SwiftUI `onDismiss` or a dismissal animation. The retained delegate handles selection/cancellation once, and busy/unavailable presentation reports an error.

Select one ZIP, then tap Open. The picker accepts generic data so that a provider's ZIP type classification does not exclude the archive; the app then requires exactly one local `.zip` URL, and the existing native importer validates the archive contents. Selecting multiple files or a different file type reports an error without importing or deleting anything. Cancellation does nothing. Selection/import errors also remain visible when game data was already installed.

**ZIP already in Muguet…** lists visible, regular `.zip` files directly in the app's Documents folder, excluding symlinks, directories and nested files. In Files, move a ZIP into **On My iPhone → Muguet**, then choose it using this launcher button. This path uses no system document picker. A ZIP already in that folder needs no move or second copy; no archive imports automatically or in bulk.

The document picker opens the selected original URL (`asCopy: false`), avoiding a second full archive copy. The import worker retains security-scoped access and uses `NSFileCoordinator` for reading and deletion. Deletion is coordinated only after successful extraction/validation/promotion and an unchanged-source identity check. Apple's [document-picker guide](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/DocumentPickerProgrammingGuide/AccessingDocuments/AccessingDocuments.html) describes modal presentation and coordinated access to external documents. The old picker stall has not been reproduced with a device trace; presentation/lifecycle is a source-based diagnosis, not a confirmed device root cause. Both new UI paths still need an iPhone check.

After extraction, validation and successful promotion of the imported game directory, the app deletes **only the selected ZIP**. The source identity is captured before importing and checked again before deletion; a changed source or failed import is retained. If Files/iCloud/provider permissions prevent deletion, game data stays ready and the launcher reports that the ZIP must be removed manually. Import deletion on a device/provider still needs acceptance.

The extracted game data continues to occupy storage. During import, both ZIP and extracted data must fit. Only the selected URL is deleted. Independent local backups remain untouched; deleting a selected iCloud/provider ZIP can also sync its deletion to that provider's other devices. Keep a separate local backup before importing on the phone.

## Personal unsigned build

Use your own original `game.wasm`; no game files or download source is included in this repository. Requires Xcode, XcodeGen, Binaryen (`wasm-opt`, tested with 133), Rust 1.98.1 and the `aarch64-apple-ios` target. The script keeps generated files in `.build`, uses two build jobs, and reserves at least 15 GiB of free space.

```sh
# The script uses local Rust/Cargo homes in .build:
RUSTUP_HOME="$PWD/.build/rustup-home" CARGO_HOME="$PWD/.build/cargo-home" \
  rustup toolchain install 1.98.1 --profile minimal --target aarch64-apple-ios
python3 tools/build-unsigned.py /path/to/your/game.wasm --xcodegen /path/to/xcodegen --wasm-opt /path/to/wasm-opt
```

Output: `.build/artifacts/performance-fix/0.1 performance fix.ipa` and SHA-256 evidence. The IPA is genuinely unsigned: signing and device installation are performed by the owner. Keep the upstream bundle identifier to preserve an existing installation's data (subject to your signing/install tool's behavior). The app retains the 5 GiB iOS heap ceiling and does not enable JIT. A current 3072 MiB heap means a 3 GiB linear-memory address range, not a 3 GiB process cap or necessarily 3 GiB of resident RAM. The ceiling does not force the game to allocate or use that much memory. Import the same compatible game-data ZIP; these changes do not require editing game-data files. One new pause-state export is added to the code embedded in the personal IPA.

## iOS RAM and signing

Physical device RAM, the game's linear heap ceiling and the memory iOS permits the whole app to use are separate values. Textures, renderer, native code and other allocations share the process budget. A reported 7.5 GB budget therefore does not make a 7.5 GB game heap safe. More heap capacity alone does not improve performance or force the game to grow its current heap.

The HUD samples Apple's `os_proc_available_memory()` every update, reports remaining process headroom, and adds the process footprint to show an approximate current budget. It uses MiB (1024² bytes). This is advisory, varies during the app's lifecycle, and must be measured on the installed app rather than inferred from the phone's physical RAM or iOS version.

The local unsigned build includes a separate `Muguet-sideload.entitlements.plist` requesting `com.apple.developer.kernel.increased-memory-limit`. An unsigned IPA cannot grant an entitlement: the owner's signing tool must include it in the signature and the provisioning profile must authorize it. [GetMoreRam](https://github.com/hugeBlack/GetMoreRam) configures this capability for a sideloaded App ID; it is redundant if the signing workflow already enables it. It does not increase physical RAM or remove the game's own heap ceiling. Whether the owner's iLoader installation enables it must be checked on that device. No signing credentials or device actions are performed by this project.

Apple references: [available app memory](https://developer.apple.com/documentation/os/os_proc_available_memory), [increased memory limit](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.kernel.increased-memory-limit).

## Checks and device acceptance

`sh tools/test-native.sh` checks single-ZIP selection, app-folder discovery, coordinated reads/deletion and ZIP cleanup protection without loading a reconstruction framework. Test output/fixtures stay in `.build`. `cargo +1.98.1 test --locked --release --jobs 2 --bin muguet` checks controller filtering/mapping and original vertex shader validation after the normal uniform packing step. Setting `MUGUET_TEST_GAME_ZIP` additionally validates every original vertex shader in your own archive without modifying it.

The local release report records compilation, checksums and unsigned packaging checks. Those checks do not establish gameplay, performance improvement, controller behavior or ZIP-provider deletion on an iPhone. The previously working upstream IPA is kept as a separate rollback artifact.

## Publication boundary

The public fork/release distributes Muguet source and documentation under GPL-3.0-or-later. It does not distribute game assets, the game's Wasm, precompiled game code, a game-bearing IPA, or links to repack archives. Build your personal IPA with files you are entitled to use. The Muguet license does not grant rights to the game or its assets.
