// Explicit fixture preparation only. Inputs must be licensed QA copies, never library discovery.
import Foundation
import ImageIO
import CoreImage
import CoreGraphics
let args=CommandLine.arguments
precondition(args.count==3,"Expected licensed JPEG and new owned output directory")
let input=URL(fileURLWithPath:args[1]), out=URL(fileURLWithPath:args[2],isDirectory:true)
precondition(!FileManager.default.fileExists(atPath:out.path),"Preserve existing fixtures")
try FileManager.default.createDirectory(at:out,withIntermediateDirectories:false)
let source=CGImageSourceCreateWithURL(input as CFURL,[kCGImageSourceShouldCache:false] as CFDictionary)!
precondition(CGImageSourceGetCount(source)==1)
let props=CGImageSourceCopyPropertiesAtIndex(source,0,nil)! as NSDictionary
let orientation=(props[kCGImagePropertyOrientation] as? NSNumber)?.int32Value ?? 1
let gain=CGImageSourceCopyAuxiliaryDataInfoAtIndex(source,0,kCGImageAuxiliaryDataTypeHDRGainMap)!
let sdr=CGImageSourceCreateImageAtIndex(source,0,[kCGImageSourceDecodeRequest:kCGImageSourceDecodeToSDR] as CFDictionary)!
let hdr=CGImageSourceCreateImageAtIndex(source,0,[kCGImageSourceDecodeRequest:kCGImageSourceDecodeToHDR] as CFDictionary)!
precondition(sdr.bitsPerComponent==8 && hdr.bitsPerComponent>8)
try FileManager.default.copyItem(at:input,to:out.appendingPathComponent("01-apple-gain.jpg"))
func save(_ name:String,_ kind:String,_ image:CGImage,_ auxiliary:CFDictionary?=nil,_ orient:Int32=orientation){
 let metadata=[kCGImagePropertyOrientation:orient,kCGImagePropertyExifDictionary:[kCGImagePropertyExifDateTimeOriginal:"2024:12:07 09:01:22"]] as CFDictionary
 let d=CGImageDestinationCreateWithURL(out.appendingPathComponent(name) as CFURL,kind as CFString,1,nil)!
 CGImageDestinationAddImage(d,image,metadata)
 if let auxiliary=auxiliary {CGImageDestinationAddAuxiliaryDataInfo(d,kCGImageAuxiliaryDataTypeHDRGainMap,auxiliary)}
 precondition(CGImageDestinationFinalize(d))
}
save("02-sdr.heic","public.heic",sdr)
save("03-apple-gain.heic","public.heic",sdr,gain)
save("04-pq.heic","public.heic",hdr)
for i in 2...8 { save("orientation-\(i).jpg","public.jpeg",sdr,gain,Int32(i)) }
// A distinct decoder/render path provides an SDR reference, without requesting HDR expansion.
let context=CIContext(options:[.workingColorSpace:CGColorSpace(name:CGColorSpace.extendedLinearSRGB)!])
let srgb=CGColorSpace(name:CGColorSpace.sRGB)!
var report:[[String:Any]]=[]
for name in ["01-apple-gain.jpg","02-sdr.heic","03-apple-gain.heic","04-pq.heic"] + (2...8).map({"orientation-\($0).jpg"}) {
 let file=out.appendingPathComponent(name)
 let src=CGImageSourceCreateWithURL(file as CFURL,nil)!
 let p=CGImageSourceCopyPropertiesAtIndex(src,0,nil)! as NSDictionary
 let cg=CGImageSourceCreateImageAtIndex(src,0,[kCGImageSourceDecodeRequest:kCGImageSourceDecodeToSDR] as CFDictionary)!
 let ci=CIImage(contentsOf:file,options:[.expandToHDR:false,.applyOrientationProperty:true])!
 let reference=context.createCGImage(ci,from:ci.extent,format:.RGBA8,colorSpace:srgb)!
 let dest=CGImageDestinationCreateWithURL(out.appendingPathComponent(name+".reference.png") as CFURL,"public.png" as CFString,1,nil)!
 CGImageDestinationAddImage(dest,reference,nil);precondition(CGImageDestinationFinalize(dest))
 let hasGain=CGImageSourceCopyAuxiliaryDataInfoAtIndex(src,0,kCGImageAuxiliaryDataTypeHDRGainMap) != nil
 report.append(["name":name,"count":CGImageSourceGetCount(src),"width":cg.width,"height":cg.height,"sourceDepth":p[kCGImagePropertyDepth] ?? 0,"profile":p[kCGImagePropertyProfileName] ?? "","appleGainMap":hasGain,"reference":"Core Image expandToHDR=false, RGBA8 sRGB", "provenance":name=="01-apple-gain.jpg" ? "unmodified licensed camera JPEG" : "generated ImageIO derivative; not an original phone file"])
}
try JSONSerialization.data(withJSONObject:report,options:[.prettyPrinted,.sortedKeys]).write(to:out.appendingPathComponent("fixture-report.json"))
print(String(data:try JSONSerialization.data(withJSONObject:report,options:[.sortedKeys]),encoding:.utf8)!)
