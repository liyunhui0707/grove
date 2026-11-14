# Photo support in 0.1.0

The public prerelease targets Apple Silicon macOS. Runtime evidence is from macOS 27.2.1 using generated fixtures, not a complete set of phone/camera models.

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
