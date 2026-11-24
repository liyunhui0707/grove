// Attach the Apple QuickTime metadata key to a locally generated video track.
import Foundation
import AVFoundation

let args = CommandLine.arguments
guard args.count == 4, let id = UUID(uuidString: args[3]),
      !FileManager.default.fileExists(atPath: args[2]),
      let export = AVAssetExportSession(asset: AVURLAsset(url: URL(fileURLWithPath: args[1])),
                                        presetName: AVAssetExportPresetPassthrough) else { exit(1) }
let item = AVMutableMetadataItem()
item.identifier = .quickTimeMetadataContentIdentifier
item.dataType = kCMMetadataBaseDataType_UTF8 as String
item.value = id.uuidString as NSString
export.metadata = [item]
export.outputURL = URL(fileURLWithPath: args[2])
export.outputFileType = .mov
let done = DispatchSemaphore(value: 0)
export.exportAsynchronously { done.signal() }
guard done.wait(timeout: .now() + 20) == .success, export.status == .completed else { exit(1) }
