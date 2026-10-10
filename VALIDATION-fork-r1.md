# Fork r1 validation — 2026-10-10

- Rust 1.98.1 / Wasmtime 49.0.2; host and `aarch64-apple-ios` release builds completed with two build jobs.
- iOS Swift sources typechecked at the iOS 15 deployment target; MetalFX is availability guarded and weak linked.
- Four Rust tests passed. They cover radial look filtering, wheel/phone/pause mappings, vertex-entry jitter rewriting, and a real GPU draw using the jitter bind group. All **1,113** vertex shaders in the owner's unchanged compatible game-data ZIP passed Metal shader validation after jitter injection.
- Native Swift tests passed: selected unchanged ZIP deletion, modified ZIP retention, and refusal to delete a non-ZIP input. The import call site invokes cleanup only after the native importer reports success.
- Actual Mac MetalFX GPU smoke test passed for three translated synthetic frames using BGRA input and depth32/stencil8. The estimated previous-frame motion was **−4 px**, and the upscaled output was non-black. GPU event synchronization was exercised by this native test. The complete wgpu/native interop path still needs iPhone acceptance.
- Personal IPA: 0.1.1, arm64, original bundle identifier `ch.cclerc.muguet.ios`, 5 GiB heap, no JIT. ZIP integrity and packaged executable equivalence checked. The executable contains the newly precompiled game code. No Mach-O signature, resource signature seal or provisioning profile remains.
- Public source scope audited: no game Wasm, precompiled game module, game assets, repack URL, personal game-bearing IPA, credential or private machine path is published. Original GPL and vendored MIT/Apache license files are retained.

The owner confirmed that the previous upstream IPA works on an iPhone. That confirmation applies to the previous build. This fork has **not** been installed on a device or tested for gameplay, frame-rate improvement, thermal behavior, every physical controller, pause/phone/radio/character-wheel behavior, or source-ZIP deletion through Files/iCloud providers. Character switching remains subject to Story Mode progress and game scripts. Controls still emulate keyboard/mouse rather than native analog steering.

The personal IPA's measured size and SHA-256 are delivered locally with `SHA256SUMS.txt`; the game-bearing binary is not attached to the public release. The compatible game-data archive and working baseline IPA were retained and verified against their existing checksums.
