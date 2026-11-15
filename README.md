# Moment Grove

给回忆，一点留白。A local-first desktop app for reviewing and organizing photos.

**0.4.0 targets technical trial users on Apple Silicon Macs.** [Website](https://moment-grove.outerepoch3.chatgpt.site) · [Releases](https://github.com/liyunhui0707/moment-grove/releases) The app interface is currently Chinese. It runs locally without an account or a separate Node.js installation.

![Illustrative continuous two-photo comparison using generated demo artwork](docs/images/comparison.png)

*Browser preview using generated demo artwork; this is not a photograph or a native macOS screenshot.*

## What you can do

- Browse ordinary folders, review photos with keyboard ratings, undo, and compare 2–8 images.
- Resume the last folder, exact photo and essential review settings after a normal quit.
- Keep comparison references visible while replacing only the active candidate with `[` / `]`.
- On macOS 14+, view tested Apple gain-map JPEG/HEIC and 10-bit PQ HEIC as SDR and export independent sRGB watermark JPEGs. HDR appearance is not retained.
- See explicit counts for skipped video, RAW and unsupported images in mixed folders.
- Read orientation-correct JPEG/HEIC dimensions and EXIF capture dates; rebuild previews with a corrected source profile for affected phone HEIC thumbnails.
- Reuse the natural-name sorter for large photo batches; distinct occurrences and rating targets remain separate.
- Keep seven distinct states: unreviewed, reject candidate (0), and ratings 1–5. A reject rating never deletes a file.
- Carry ratings in XMP sidecars, review conflicts, and create local catalog snapshots.
- Collect 4–5-star photos and export verified original-byte copies.
- Export separate sRGB JPEGs with a date and optional manually entered city inside the photo.
- Review same-volume moves and low-rating isolation, with durable logs and reviewed withdrawal.

## Download and install

Download the exact macOS arm64 ZIP or DMG and `SHA256SUMS.txt` from [0.4.0 test prerelease](https://github.com/liyunhui0707/moment-grove/releases/tag/v0.4.0). Both archives contain the same app, user guide and license notices. Check the archive before opening it:

```sh
shasum -a 256 --check --strict --ignore-missing SHA256SUMS.txt
```

Keep the checksum file and the chosen ZIP or DMG in the same folder. Require `OK` and a successful exit; if no file is verified or a checksum fails, stop without extracting or running it. Checksums detect changes; they are not an independent developer signature.

Open the DMG and drag **Moment Grove.app** to Applications, or extract the ZIP and copy the app. Quit any older copy before replacing it.

**Signing:** ad-hoc signed, without Developer ID or Apple notarization. macOS may block the first opening. Follow [Apple's instructions for opening software from an unidentified developer](https://support.apple.com/guide/mac-help/open-a-mac-app-from-an-unidentified-developer-mh40616/mac) only if you trust the source. No security-disabling commands are required. A browser-download Gatekeeper flow has not been accepted as part of this prerelease.

**Native upgrade check:** the exact released 0.3.0 App established ratings and a real snapshot in an isolated copy-set. Replacing it with 0.4.0 at the same install/data paths retained identities, ratings and pending writes. The final candidate completed native original-byte copies and full-size watermark exports (two each), then quit normally. A separate eight-photo delivery scene verified normal restart at the exact HEIC photo and old-version rollback with the new rating preserved. All three uploaded assets were fully downloaded and matched the frozen hashes. The installed downloaded App restored the exact HEIC position and settings and quit normally; details and earlier interrupted capture attempts are recorded in the [build manifest](docs/build-manifest.json). Browser-quarantine/Gatekeeper first opening remains unaccepted.

**System scope:** arm64 / Apple Silicon; tested on macOS 27.2.1. The bundle declares macOS 12 as its minimum, but other OS versions, Intel Macs, Windows and Linux have not been validated for this release.

## First use

Choose **体验示例图库** for generated demo photos, or open a folder of disposable test copies. This is an early prerelease: keep independent media backups and use test copies for file operations.

Use 0–5 to rate, U to clear, ⌘Z to undo, and C to compare a selection. Ratings save to the local catalog; **评分同步** explicitly writes XMP. **导出副本** preserves original bytes and metadata. Watermark exports omit the source EXIF/GPS. Review every file-operation plan before confirming.

See [installation, upgrade and rollback](docs/installation-upgrade.md), the [user guide](docs/user-guide.md), [format support](docs/format-support.md), and [release notes](docs/release-notes.md).

## Privacy, updates and recovery

The app does not upload photos, require an account, query online city services, or perform automatic updates. System file pickers determine which folders it can use. Catalog snapshots contain ratings/catalog data, not photo backups.

Update manually: create a catalog snapshot, quit the app, keep the old app, and replace it. Withdrawal must precede catalog restoration when safe moves exist. Rollback is limited to the exact pairs and data described in the release evidence; arbitrary version rollback is not promised. Retain the old app and an independent catalog snapshot before replacing it. Uninstalling the app does not automatically erase the photo folder or application data.

## Build from source

Use an existing Rust toolchain, a Node.js version satisfying the locked Vite package's engine requirement, and the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
npm run desktop
```

Static checks and tests:

```sh
npm run build
npm test
npm run test:rust
npm run test:ui
npm run check:rust
node --test scripts/tests/release-version.test.mjs
```

Browser checks require installed Google Chrome. macOS decoder tests use system ImageIO/ColorSync and generated fixtures; the optional mobile matrix additionally uses attributed samples and a separate Core Image SDR reference; other platforms are not supported by this release. Performance and destructive-fault helper cases are opt-in and should only run in an owned test environment.

To build the macOS arm64 prerelease archives:

```sh
npm run desktop:package -- --prerelease
```

Packaging writes to ignored `.local/deliveries/`, uses an offline Cargo build after dependencies are available, and performs no upload. The package has no automatic-update service. There are no GitHub Actions workflows that publish on push or tags.

## Feedback and license

Report reproducible problems in [Issues](https://github.com/liyunhui0707/moment-grove/issues). Include the app version, macOS version and steps. Remove private paths, photo metadata and personal images before sharing logs or screenshots. See [security reporting](SECURITY.md) for sensitive reports.

Project code is [Apache-2.0](LICENSE). Dependencies retain their own licenses; full texts and source links are in [third-party notices](THIRD-PARTY-NOTICES.txt). macOS system frameworks and fonts are not bundled. No libheif, FFmpeg, cloud service or AI model is shipped.
