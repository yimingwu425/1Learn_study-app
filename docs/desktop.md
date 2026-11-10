# Desktop shell (Tauri v2) — current Mac development build

Use `启动桌面学习软件.command`. Rust owns an OS-selected loopback core, waits for readiness, and reaps that child on normal quit/startup failure. The same React UI is used in Tauri and browser. No Tauri IPC grants or off-origin navigation were added.

The current Mac build has been compiled and actually opened, operated and restarted. Core data defaults to this independent project's data directory for debug launchers. Release builds use the Tauri application-data directory unless explicitly configured. Never share a test database with the user app.

The launcher and runtime no longer check, configure or run Python. PDF operations use the official project-local PdfCraft CLI and native Rust modules. Existing local Tesseract is used for damaged-text export boundaries; see native-pdf-runtime.md for paths, configuration and licenses. Node/npm is needed for frontend development, not a running built app.

Development: `scripts/desktop-dev.sh`. Mac resource staging: `scripts/desktop-stage.sh`, then the Tauri bundle overlay. Staging copies native engine/declared assets, not Python scripts or the user's database. The staged target must match the host; the checked-in official PdfCraft CLI is macOS universal. Tesseract is not yet bundled.

`Study Local.app` remains a development artifact. Signing, notarization, self-contained OCR packaging and installer validation are unfinished. A successful runtime test is not a signed-distribution claim.

Standalone ownership tests:

```sh
rustc --edition=2021 --test desktop/src/runtime.rs -o /tmp/study-desktop-runtime-tests
/tmp/study-desktop-runtime-tests
```

Full native checking uses `cargo check --manifest-path desktop/Cargo.toml` and requires the target OS's Tauri platform libraries. Forced termination is not guaranteed to run graceful child cleanup.
