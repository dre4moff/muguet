import Foundation
import Metal
import MetalFX

func require(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() { fatalError(message) }
}

let directory = FileManager.default.temporaryDirectory.appendingPathComponent("muguet-native-tests-\(UUID().uuidString)")
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: directory) }
let original = directory.appendingPathComponent("original.zip")
try Data([1,2,3]).write(to: original)
let identity = try SourceArchiveIdentity.read(original)
try identity.removeUnchangedArchive(at: original)
require(!FileManager.default.fileExists(atPath: original.path), "Successful import cleanup must delete selected ZIP")
try Data([1,2,3]).write(to: original)
let changed = try SourceArchiveIdentity.read(original)
try Data([1,2,3,4]).write(to: original)
do { try changed.removeUnchangedArchive(at: original); fatalError("Changed source ZIP was deleted") }
catch ArchiveCleanupError.changed { }
require(FileManager.default.fileExists(atPath: original.path), "Changed source must survive")
let nonZIP = directory.appendingPathComponent("data.txt")
try Data([1]).write(to: nonZIP)
do { _ = try SourceArchiveIdentity.read(nonZIP); fatalError("Non-ZIP accepted for deletion") }
catch ArchiveCleanupError.identityUnavailable { }
print("PASS: selected unchanged ZIP cleanup; modified ZIP and non-ZIP protection")

if #available(macOS 13.0, *), let device = MTLCreateSystemDefaultDevice(), MTLFXTemporalScalerDescriptor.supportsDevice(device) {
    for ratio in [1.5, 2.0] {
        let width = 128, height = 96
        let outputWidth = Int(Double(width) * ratio), outputHeight = Int(Double(height) * ratio)
        func texture(_ format: MTLPixelFormat, _ w: Int, _ h: Int, _ usage: MTLTextureUsage, shared: Bool = false) -> MTLTexture {
            let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: format, width: w, height: h, mipmapped: false)
            desc.usage = usage; desc.storageMode = shared ? .shared : .private
            return device.makeTexture(descriptor: desc)!
        }
        let color = texture(.bgra8Unorm, width, height, [.shaderRead], shared: true)
        let depth = texture(.depth32Float_stencil8, width, height, [.shaderRead,.renderTarget])
        let output = texture(.rgba8Unorm, outputWidth, outputHeight, [.shaderRead,.shaderWrite,.renderTarget])
        guard let fx = TemporalUpscaler(device: device, output: output, width: width, height: height) else { fatalError("Supported MetalFX initialization failed") }
        let queue = device.makeCommandQueue()!
        for frame in 0..<3 {
            var pixels = [UInt8](repeating: 255, count: width*height*4)
            for y in 0..<height { for x in 0..<width {
                let previousX = max(0,x-frame*4)
                let value = UInt8((previousX*17 + y*37 + (previousX/9)*(y/7)*19) % 220 + 20)
                for c in 0..<3 { pixels[(y*width+x)*4+c] = value }
            } }
            pixels.withUnsafeBytes { color.replace(region: MTLRegionMake2D(0,0,width,height), mipmapLevel: 0, withBytes: $0.baseAddress!, bytesPerRow: width*4) }
            let producer = queue.makeCommandBuffer()!
            let pass = MTLRenderPassDescriptor()
            pass.depthAttachment.texture = depth; pass.depthAttachment.loadAction = .clear
            pass.depthAttachment.storeAction = .store; pass.depthAttachment.clearDepth = 0.6
            producer.makeRenderCommandEncoder(descriptor: pass)!.endEncoding()
            let serial = UInt64(frame+1)
            producer.encodeSignalEvent(fx.event, value: serial*2-1); producer.commit()
            require(fx.encode(source: color, sceneDepth: depth, jitter: .zero, reversed: false, serial: serial, reset: frame==0), "Temporal encode failed")
            let read = queue.makeCommandBuffer()!
            read.encodeWaitForEvent(fx.event, value: serial*2)
            let outputBytes = outputWidth * outputHeight * 4
            let pixelsOut = device.makeBuffer(length: outputBytes, options: .storageModeShared)!
            let vectors = device.makeBuffer(length: width*height*4, options: .storageModeShared)!
            let blit = read.makeBlitCommandEncoder()!
            blit.copy(from: output, sourceSlice: 0, sourceLevel: 0, sourceOrigin: .init(x:0,y:0,z:0), sourceSize: .init(width:outputWidth,height:outputHeight,depth:1), to: pixelsOut, destinationOffset: 0, destinationBytesPerRow: outputWidth*4, destinationBytesPerImage: outputBytes)
            blit.copy(from: fx.motion, sourceSlice: 0, sourceLevel: 0, sourceOrigin: .init(x:0,y:0,z:0), sourceSize: .init(width:width,height:height,depth:1), to: vectors, destinationOffset: 0, destinationBytesPerRow: width*4, destinationBytesPerImage: width*height*4)
            blit.endEncoding(); read.commit(); read.waitUntilCompleted()
            require(read.status == .completed && producer.status == .completed, "GPU command buffer error")
            require(pixelsOut.contents().assumingMemoryBound(to: UInt8.self)[outputBytes/2] > 0, "Black temporal output")
            if frame > 0 {
                let data = vectors.contents().assumingMemoryBound(to: UInt16.self)
                let flow = Float(Float16(bitPattern: data[(height/2*width+width/2)*2]))
                require(flow < -1 && flow > -8, "Estimated motion must point to previous frame, observed \(flow)")
                print("PASS: actual MetalFX \(ratio)× GPU output \(outputWidth)×\(outputHeight) and previous-frame motion: \(flow) px")
            }
        }
    }
} else { print("SKIP: MetalFX GPU smoke (unsupported host)") }
