// Generates an ImageIO MakerApple 17 fixture from a synthetic PNG.
import Foundation
import ImageIO

let args = CommandLine.arguments
guard args.count == 4, let uuid = UUID(uuidString: args[3]),
      let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: args[1]) as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
      !FileManager.default.fileExists(atPath: args[2]),
      let output = CGImageDestinationCreateWithURL(URL(fileURLWithPath: args[2]) as CFURL,
                                                   "public.jpeg" as CFString, 1, nil) else { exit(1) }
let metadata: [String: Any] = [
    kCGImagePropertyTIFFDictionary as String: [kCGImagePropertyTIFFMake as String: "Apple",
                                               kCGImagePropertyTIFFModel as String: "Generated QA fixture"],
    kCGImagePropertyMakerAppleDictionary as String: ["17": uuid.uuidString]
]
CGImageDestinationAddImage(output, image, metadata as CFDictionary)
guard CGImageDestinationFinalize(output) else { exit(1) }
