// Explicit prepared copy-set only. Never upload outputs or discover personal folders.
import Foundation
import CoreImage
import ImageIO
import CoreGraphics
let root=URL(fileURLWithPath:CommandLine.arguments[1],isDirectory:true)
let m=try JSONSerialization.jsonObject(with:Data(contentsOf:root.appendingPathComponent("fixtures.json"))) as! [String:Any]
let out=root.appendingPathComponent("references",isDirectory:true)
precondition(!FileManager.default.fileExists(atPath:out.path),"Keep previous reference evidence")
try FileManager.default.createDirectory(at:out,withIntermediateDirectories:false)
let srgb=CGColorSpace(name:CGColorSpace.sRGB)!,context=CIContext(options:[.workingColorSpace:CGColorSpace(name:CGColorSpace.extendedLinearSRGB)!])
let names=m["referenceNames"] as! [String]
for name in names {
 precondition(URL(fileURLWithPath:name).lastPathComponent==name && name.hasPrefix("sample-"))
 let source=root.appendingPathComponent("sources").appendingPathComponent(name)
 let image=CIImage(contentsOf:source,options:[.expandToHDR:false,.applyOrientationProperty:true])!
 precondition(image.extent.width*image.extent.height<=24_000_000)
 // Preserve native pixels; the Rust comparison applies the same explicit
 // downsampler to the reference, avoiding a second, linear-light CI resize.
 let cg=context.createCGImage(image,from:image.extent,format:.RGBA8,colorSpace:srgb)!
 let dest=CGImageDestinationCreateWithURL(out.appendingPathComponent(name+".png") as CFURL,"public.png" as CFString,1,nil)!
 CGImageDestinationAddImage(dest,cg,nil);precondition(CGImageDestinationFinalize(dest))
}
print("Independent oriented SDR references: \(names.count); personal pixels retained locally")
