# Muguet fork r2 (0.1.2)

This fork starts from c22dev/Muguet commit `63737647eb3f7fee2e2139995606e3b02a5920ee`. The original author, copyright notices, GPL-3.0-or-later license, and vendored MIT/Apache licenses are retained. This is an independent modification by dre4moff, not an official Rockstar or upstream release.

## Launcher options

- **MetalFX Temporal (experimental)**: disabled by default; available on supported devices running iOS 16 or later. Render resolution still follows the existing resolution scale; A launcher selector reconstructs at **1.5× or 2× the selected render dimensions** (default 1.5×). For example, 1280×720 stays the game input and produces 1920×1080 or 2560×1440. It does not automatically upscale to the physical display dimensions or divide the selected input by the multiplier. The output is fitted to the display; multiplying output dimensions does not create native-rendered detail. An unsupported texture size falls back without lowering the input. The iOS input stays fixed for the session, independently of drawable resizing. The renderer supplies scene depth, an eight-frame sampling jitter, and estimated previous-frame image motion. It does not have engine-provided geometry motion vectors. Fast motion, disocclusions, scene cuts, transparency, and game UI can produce trails or other artifacts. GPU events order the renderer, native scaler, and presentation without a per-frame CPU wait. Missing scene depth uses the ordinary presentation path; initialization or encoding failure falls back. The HUD reports the current status.
- **Performance HUD**: disabled by default. Displays visible presentation FPS, average presentation interval (not GPU execution time), process memory footprint, remaining iOS process memory budget, approximate total process budget, current linear heap / ceiling, render/output sizes, draw count, graphics validation errors, controller connection, and thermal state. It updates twice per second and does not intercept touch input.
- **Physical controller**: separate look sensitivity (0.25–4, default 1.5) and radial deadzone (5–30%, default 12%). The right-stick response is independent of touch and mouse sensitivity. Apple Game Controller's extended gamepad profile is used; individual hardware has not been certified. r2 retains r1 controller code: the owner confirmed the controller is connected and works correctly, so no new controller changes were made.

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

After extraction, validation and successful promotion of the imported game directory, the app deletes **only the selected ZIP**. The source identity is captured before importing and checked again before deletion; a changed source or failed import is retained. If Files/iCloud/provider permissions prevent deletion, game data stays ready and the launcher reports that the ZIP must be removed manually. Import deletion on a device/provider still needs acceptance.

The extracted game data continues to occupy storage. During import, both ZIP and extracted data must fit. Only the selected URL is deleted. Independent local backups remain untouched; deleting a selected iCloud/provider ZIP can also sync its deletion to that provider's other devices. Keep a separate local backup before importing on the phone.

## Personal unsigned build

Use your own original `game.wasm`; no game files or download source is included in this repository. Requires Xcode, XcodeGen, Rust 1.98.1 and the `aarch64-apple-ios` target. The script keeps generated files in `.build`, uses two build jobs, and reserves at least 15 GiB of free space.

```sh
# The script uses local Rust/Cargo homes in .build:
RUSTUP_HOME="$PWD/.build/rustup-home" CARGO_HOME="$PWD/.build/cargo-home" \
  rustup toolchain install 1.98.1 --profile minimal --target aarch64-apple-ios
python3 tools/build-unsigned.py /path/to/your/game.wasm --xcodegen /path/to/xcodegen
```

Output: `.build/artifacts/fork-r2/Muguet-0.1.2-fork-r2-ios-arm64-unsigned.ipa` and SHA-256 evidence. The IPA is genuinely unsigned: signing and device installation are performed by the owner. Keep the upstream bundle identifier to preserve an existing installation's data (subject to your signing/install tool's behavior). The app retains the 5 GiB iOS heap ceiling and does not enable JIT. A current 3072 MiB heap means a 3 GiB linear-memory address range, not a 3 GiB process cap or necessarily 3 GiB of resident RAM. The ceiling does not force the game to allocate or use that much memory. Import the same compatible game-data ZIP; these changes do not require editing game-data files. One new pause-state export is added to the code embedded in the personal IPA.

## iOS RAM and signing

Physical device RAM, the game's linear heap ceiling and the memory iOS permits the whole app to use are separate values. Textures, renderer, MetalFX history, native code and other allocations share the process budget. A reported 7.5 GB budget therefore does not make a 7.5 GB game heap safe. More heap capacity alone does not improve performance or force the game to grow its current heap.

The HUD samples Apple's `os_proc_available_memory()` every update, reports remaining process headroom, and adds the process footprint to show an approximate current budget. It uses MiB (1024² bytes). This is advisory, varies during the app's lifecycle, and must be measured on the installed app rather than inferred from the phone's physical RAM or iOS version.

The local unsigned build includes a separate `Muguet-sideload.entitlements.plist` requesting `com.apple.developer.kernel.increased-memory-limit`. An unsigned IPA cannot grant an entitlement: the owner's signing tool must include it in the signature and the provisioning profile must authorize it. [GetMoreRam](https://github.com/hugeBlack/GetMoreRam) configures this capability for a sideloaded App ID; it is redundant if the signing workflow already enables it. It does not increase physical RAM or remove the game's own heap ceiling. Whether the owner's iLoader installation enables it must be checked on that device. No signing credentials or device actions are performed by this project.

Apple references: [available app memory](https://developer.apple.com/documentation/os/os_proc_available_memory), [increased memory limit](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.kernel.increased-memory-limit).

## Checks and device acceptance

`sh tools/test-native.sh` checks ZIP cleanup protection and executes both MetalFX ratios on a supported Mac GPU with translated synthetic frames. `cargo +1.98.1 test --locked --release --jobs 2 --bin muguet` checks selected-resolution reconstruction dimensions, texture limits, controller filtering/mapping and jitter shader validation. Setting `MUGUET_TEST_GAME_ZIP` additionally validates every vertex shader in your own archive without modifying it.

The local release report records compilation, checksums and unsigned packaging checks. Those checks do not establish gameplay, performance improvement, controller behavior or ZIP-provider deletion on an iPhone. The previously working upstream IPA is kept as a separate rollback artifact.

## Publication boundary

The public fork/release distributes Muguet source and documentation under GPL-3.0-or-later. It does not distribute game assets, the game's Wasm, precompiled game code, a game-bearing IPA, or links to repack archives. Build your personal IPA with files you are entitled to use. The Muguet license does not grant rights to the game or its assets.
