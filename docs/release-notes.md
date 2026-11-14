# 0.1.0 — public prerelease

First public source and macOS Apple Silicon archives. Ad-hoc signed; not notarized. No automatic updates.

Includes folder browsing, persistent seven-state ratings and undo, 2–8-photo comparison, XMP sync/conflict review, catalog snapshots, original-byte copy exports, date/city watermark derivatives, and reviewed same-volume moves/isolation/withdrawal.

Preview compatibility improvements normalize SDR colour to sRGB, composite transparency over white, apply all eight EXIF orientations once, distinguish the corrected preview cache, reject detected unsupported HDR/high-depth/multi-frame cases, and provide a retry action.

Validation uses generated media on a second Apple Silicon Mac: 90 backend checks, a generated eight-photo 12MP cold/warm preview baseline, and 24 browser checks with mocked native IPC where indicated. Previous local-trial native acceptance covered archive installation, ratings/sync, exports, isolation/restart/withdrawal, manual snapshots and compatible-candidate rollback. These are distinct evidence sets; browser mocks do not prove native picker or WebView behaviour. Latest package acceptance is recorded in the release description.

Known boundaries: generated HEIC/P3 samples only, no guarantee for arbitrary camera HDR; no complete Live Photo/RAW resource-group operations, cross-volume moves, Windows/Linux packages, Intel package, public historical-version migration guarantee, or validated browser-download Gatekeeper flow. Real disk failure/power-loss testing and long-running full-App large-library measurements remain future work.

The bundled SQLite dependency was updated to rusqlite 0.41.1 / libsqlite3-sys 0.39.1 before publication, addressing [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html). The application catalog remains schema 2. Advisory checks of the fixed macOS Rust graph and production npm dependencies returned no flagged packages on 2026-11-14.

## Exact-package native check boundary

The final App was independently extracted from ZIP and mounted read-only from DMG on the test Mac; bundle resources, signature and bytes matched. It started with isolated application data. Screen Sharing control then failed before the additional mixed-format/retry desktop walkthrough could be completed. The test App was stopped with a scoped SIGTERM; this is not a normal menu-quit acceptance. No additional native walkthrough PASS is claimed for this prerelease. The new decoder/colour path is covered by real system-decoder backend tests, while retry UI checks use browser IPC mocks.

The final archive repack adds the linked format guide and normalizes notice whitespace; its entire App bundle is byte-identical to the archive-installed App. No browser-download Gatekeeper acceptance, notarization or formal stable-release acceptance is claimed. Start with disposable photo copies.
