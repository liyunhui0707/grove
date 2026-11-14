# Photo support in 0.2.0

The public prerelease targets Apple Silicon macOS. Runtime evidence is from macOS 27.2.1 using generated fixtures and the explicitly attributed samples below, not a complete set of phone/camera models. The audience remains technical trial users.

| Input | Browsing | Watermark export |
|---|---|---|
| Static 8-bit JPEG / PNG / WebP | System ImageIO decode; orientation applied once; sRGB previews | Independent sRGB JPEG, subject to the 24MP / 32MiB / 100-photo limits |
| Single-frame 8-bit SDR HEIC/HEIF | System ImageIO; generated SDR HEIC tested | Generated SDR HEIC tested; same limits |
| Embedded Display P3 SDR | Converted to sRGB for preview; generated sample compared to system ColorSync output | System colour-managed SDR conversion; camera-specific colour fidelity is not comprehensively validated |
| Transparency | Flattened over white for JPEG previews | Flattened over white for JPEG derivatives |
| HDR, high bit depth, multi-frame input | Detected cases are rejected with a message; no HDR rendering promise | Unsupported cases are rejected; no tone-mapping promise |
| Live Photos / RAW pairs | Full resource-group handling is not implemented | Paired file operations are blocked rather than split |

Previews: 128MiB source limit, at most 64MP / 30,000px per source edge; generated preview bitmap at most 24MP. Grid previews use 480px, regular panes 1600px, one requested detail pane up to 8192px. The detail request may be refused when its pixel budget is exceeded. The backend bounds decode workers to two and queue slots to 24; frontend fetch concurrency is four. A failed preview has a retry action and does not prevent reviewing other files.

These are operational limits, not promises that every source below them will decode. Metadata/profile-based HDR detection does not certify all proprietary HDR variants. Unknown camera-specific colour behaviour remains unverified; originals are not rewritten by browsing or derivative export. Copy exports retain source metadata, so choose watermark derivatives when source EXIF/GPS should be omitted.

The implementation uses [ImageIO thumbnails](https://developer.apple.com/documentation/imageio/cgimagesourcecreatethumbnailatindex(_:_:_:)) and an sRGB CoreGraphics canvas. libheif was considered but is not bundled; a system decoder avoids shipping another codec stack for this macOS-only prerelease.

## Attributed device samples

Three original licensed JPEG files are retained outside the repository, with source-page, author, license and SHA-256 receipts. They are QA inputs, not bundled product media. A device name describes only the tested file, not every file from that device.

| Source and license | EXIF camera | Observed scope |
|---|---|---|
| [Apple IPhone 2024](https://commons.wikimedia.org/wiki/File:Apple_IPhone_2024.jpg), OneTrueKingLives, [CC0](https://creativecommons.org/publicdomain/zero/1.0/) | Apple iPhone 13 | HDR gain map detected: explicit preview/watermark refusal; raw copy remains byte-identical |
| [IPhone 13, January 28, 2024](https://commons.wikimedia.org/wiki/File:IPhone_13,_January_28,_2024.jpg), メイド理世, [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) | MEIZU 20 (the subject is an iPhone) | SDR preview and watermark; unequal horizontal/vertical print DPI exposed a pixel-aspect bug, now regression-covered |
| [Hopetoun falls](https://commons.wikimedia.org/wiki/File:Hopetoun_falls.jpg), DAVID ILIFF, [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/) | Canon EOS 10D | SDR preview, watermark and byte-identical raw copy |

ImageIO's [thumbnail transform](https://developer.apple.com/documentation/imageio/kcgimagesourcecreatethumbnailwithtransform) can also scale for aspect information. For unequal print DPI, this app decodes without that transform and applies EXIF orientation to the pixel canvas itself. All eight orientations are checked with generated asymmetric corner fixtures; preview cache generation changed so an older distorted thumbnail is not reused. Existing pixel/memory and full-size watermark checks remain in force.
