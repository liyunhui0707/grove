# Photo support in 0.8.0

The public prerelease targets Apple Silicon macOS. Runtime evidence is from macOS 27.2.1 using generated fixtures and the explicitly attributed samples below, not a complete set of phone/camera models. The audience remains technical trial users.

| Input | Browsing | Watermark export |
|---|---|---|
| Static 8-bit JPEG / PNG / WebP | System ImageIO decode; orientation applied once; sRGB previews | Independent sRGB JPEG, subject to the 24MP / 32MiB / 100-photo limits |
| Single-frame 8-bit SDR HEIC/HEIF | System ImageIO; generated SDR HEIC tested | Generated SDR HEIC tested; same limits |
| Embedded Display P3 SDR | Converted to sRGB for preview; generated sample compared to system ColorSync output | System colour-managed SDR conversion; camera-specific colour fidelity is not comprehensively validated |
| Transparency | Flattened over white for JPEG previews | Flattened over white for JPEG derivatives |
| Apple gain-map JPEG/HEIC and tested 10-bit PQ HEIC, macOS 14+ | Explicit system SDR decode, then 8-bit sRGB preview; no HDR display | Independent 8-bit sRGB JPEG; HDR effect is not retained |
| Other high bit depth / multi-frame input, or HDR on systems without SDR decode | Refused with a specific message | Refused; no silent clipping or unsupported conversion |
| Recognized Live Photo markers / same-name video or RAW companions | Static photo review with a resource label; no motion playback | Independent static watermark JPEG allowed with explicit scope; original pair copy only for a unique same-stem JPEG/HEIC + MOV with matching Apple identifiers; group move and Trash blocked |

Previews: 128MiB source limit, at most 64MP / 30,000px per source edge; generated preview bitmap at most 24MP. Grid previews use 480px, regular panes 1600px, one requested detail pane up to 8192px. The detail request may be refused when its pixel budget is exceeded. The backend bounds decode workers to two and queue slots to 24; frontend fetch concurrency is four. A failed preview has a retry action and does not prevent reviewing other files.

These are operational limits, not promises that every source below them will decode. Metadata/profile-based HDR detection does not certify all proprietary HDR variants. Unknown camera-specific colour behaviour remains unverified; originals are not rewritten by browsing or derivative export. Copy exports retain source metadata, so choose watermark derivatives when source EXIF/GPS should be omitted.

The implementation uses [ImageIO thumbnails](https://developer.apple.com/documentation/imageio/cgimagesourcecreatethumbnailatindex(_:_:_:)) and an sRGB CoreGraphics canvas. libheif was considered but is not bundled; a system decoder avoids shipping another codec stack for this macOS-only prerelease.

## Attributed device samples

Three original licensed JPEG files are retained outside the repository, with source-page, author, license and SHA-256 receipts. They are QA inputs, not bundled product media. A device name describes only the tested file, not every file from that device.

| Source and license | EXIF camera | Observed scope |
|---|---|---|
| [Apple IPhone 2024](https://commons.wikimedia.org/wiki/File:Apple_IPhone_2024.jpg), OneTrueKingLives, [CC0](https://creativecommons.org/publicdomain/zero/1.0/) | Apple iPhone 13 | Apple gain-map JPEG: SDR preview/watermark verified on macOS 27.2.1; raw copy remains byte-identical |
| [IPhone 13, January 28, 2024](https://commons.wikimedia.org/wiki/File:IPhone_13,_January_28,_2024.jpg), メイド理世, [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) | MEIZU 20 (the subject is an iPhone) | SDR preview and watermark; unequal horizontal/vertical print DPI exposed a pixel-aspect bug, now regression-covered |
| [Hopetoun falls](https://commons.wikimedia.org/wiki/File:Hopetoun_falls.jpg), DAVID ILIFF, [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/) | Canon EOS 10D | SDR preview, watermark and byte-identical raw copy |

ImageIO's [thumbnail transform](https://developer.apple.com/documentation/imageio/kcgimagesourcecreatethumbnailwithtransform) can also scale for aspect information. For unequal print DPI, this app decodes without that transform and applies EXIF orientation to the pixel canvas itself. All eight orientations are checked with generated asymmetric corner fixtures; preview cache generation changed so an older distorted thumbnail is not reused. Existing pixel/memory and full-size watermark checks remain in force.

## 0.3.0 SDR compatibility matrix

The unmodified CC0 iPhone 13 JPEG above and ten derivatives generated with ImageIO are tested outside source control: normal SDR HEIC, Apple gain-map HEIC, 10-bit PQ HEIC, and seven gain-map JPEG orientation variants (2–8). The HEIC files are generated derivatives, not original phone HEIC captures. Core Image with `expandToHDR=false`, orientation applied and RGBA8/sRGB output provides an independent reference. Per-image pixel differences, dimensions and source hashes are recorded in the release evidence; this is not a visual-fidelity guarantee for every camera.

ImageIO's [explicit SDR decode request](https://developer.apple.com/documentation/imageio/kcgimagesourcedecodetosdr) is available from macOS 14. Constants are looked up at runtime to preserve ordinary SDR use on earlier declared systems; an unavailable HDR decoder produces an error. Only macOS 27.2.1/arm64 was actually tested. Unknown high-depth PNG, multi-frame content, true HDR rendering, Live Photo playback and RAW processing remain unsupported. HEIC listing dimensions and EXIF dates now come from ImageIO metadata without decoding the full image.

## 0.4.0 everyday-photo trial

Mixed-folder completion reports counts for skipped video, RAW and unsupported image formats; it does not decode or import those resources. Search and rating filters do not change these whole-folder counts. JPEG listing dimensions follow EXIF orientation. For unnamed thumbnail transfer profiles, the macOS preview uses the full SDR image’s actual profile before drawing into sRGB, keeping the original unchanged. Prior cached previews are rebuilt under a new generation. Personal trial fixtures are never included in this repository, downloads or the website.

## 0.7.0 verification and folder boundaries

The same three attributed originals were downloaded again and verified against their recorded SHA-256 identities. The 11-file mobile SDR matrix and six-file camera/conversion trial were rerun on the local Mac; generated HEIC/HDR variants remain derivatives, not original device captures. Corrupt-input isolation, partial permission errors, eight orientations, repeated scans and output recovery remain regression-covered. This does not expand the list of certified devices or operating systems.

When opening a parent folder, standard Grove copy, watermark and move batch subdirectories are excluded, alongside the existing quarantine/hidden exclusions. Opening the output folder itself remains supported. This prevents generated outputs from silently becoming new review candidates; similarly named ordinary albums are retained.

## 0.8.0 original HEIC/HEIF and association scope

Three unmodified files from [ianare/exif-samples](https://github.com/ianare/exif-samples/tree/5332a9cf0220e9c6c93f88a187daa90808051e10/heic/mobile) are tested privately under the repository's [CC BY-SA4.0 user-contribution terms](https://github.com/ianare/exif-samples/blob/5332a9cf0220e9c6c93f88a187daa90808051e10/README.rst). Their bytes and Git blob identities are pinned; they are not distributed in this product.

- `iphone_13_pro_max.HEIC`, contributed by [manand881](https://github.com/ianare/exif-samples/commit/f0462fcc42f7bad484fe637389b734612d97041f): ImageIO reports iPhone13ProMax, orientation6, capture date2022-02-16, displayed3024×4032.
- `HMD_Nokia_8.3_5G.heif` and `HMD_Nokia_8.3_5G_hdr.heif`, exif-samples contributors: filenames describe the sample source; ImageIO exposes no model or original-date field in these files. Displayed sizes are3456×4608 and4608×3456. The `_hdr` filename alone is not HDR certification.

Six previews, three byte-identical copies, three2048px watermarks, manual fallback for absent dates, restarted ratings and source hashes passed the explicit backend trial on macOS27.2.1. No new device-wide or operating-system certification is implied.

Association detection indexes same-directory basenames without quadratic rescanning, including all video/RAW suffixes reported by the scanner. Apple MakerNote content identifiers are also conservatively recognized using ImageIO's MakerApple17 field ([tag definition](https://exiftool.org/TagNames/Apple.html)); identifier values are not displayed or logged. A generated marker fixture verifies protection even with renamed/absent video; it is not a real captured Live Photo. Original copy accepts a unique same-directory, same-stem still and MOV with matching MakerApple 17 and QuickTime content UUIDs; group moves remain blocked, while independent static watermark derivatives are allowed with an explicit scope note. Metadata removed or rewritten by other software, arbitrary cross-directory relationships, full motion playback and groups containing other resources remain outside this support scope.
