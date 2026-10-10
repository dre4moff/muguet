// GPL-3.0-or-later. Experimental image-motion reconstruction for MetalFX.
import Foundation
import Metal
import MetalFX

@available(iOS 16.0, macOS 13.0, *)
final class TemporalUpscaler {
    let device: MTLDevice
    let queue: MTLCommandQueue
    let event: MTLSharedEvent
    let scaler: MTLFXTemporalScaler
    let color, previous, depth, motion, coarse: MTLTexture
    let convert, estimate, expand: MTLComputePipelineState
    var history = false
    var lastJitter = SIMD2<Float>(repeating: 0)
    var lastTime = ProcessInfo.processInfo.systemUptime
    private let stateLock = NSLock()
    private var failed = false

    init?(device: MTLDevice, output: MTLTexture, width: Int, height: Int) {
        guard MTLFXTemporalScalerDescriptor.supportsDevice(device),
              let queue = device.makeCommandQueue(), let event = device.makeSharedEvent() else { return nil }
        let desc = MTLFXTemporalScalerDescriptor()
        desc.inputWidth = width; desc.inputHeight = height
        desc.outputWidth = output.width; desc.outputHeight = output.height
        desc.colorTextureFormat = .rgba8Unorm
        desc.depthTextureFormat = .r32Float
        desc.motionTextureFormat = .rg16Float
        desc.outputTextureFormat = .rgba8Unorm
        desc.isAutoExposureEnabled = false
        guard let scaler = desc.makeTemporalScaler(device: device) else { return nil }
        func texture(_ format: MTLPixelFormat, _ w: Int, _ h: Int, _ usage: MTLTextureUsage) -> MTLTexture? {
            let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: format, width: w, height: h, mipmapped: false)
            d.usage = usage.union([.shaderRead, .shaderWrite]); d.storageMode = .private
            return device.makeTexture(descriptor: d)
        }
        guard let color = texture(.rgba8Unorm, width, height, scaler.colorTextureUsage),
              let previous = texture(.rgba8Unorm, width, height, [.shaderRead]),
              let depth = texture(.r32Float, width, height, scaler.depthTextureUsage),
              let motion = texture(.rg16Float, width, height, scaler.motionTextureUsage),
              let coarse = texture(.rg16Float, (width + 7) / 8, (height + 7) / 8, [.shaderRead]),
              let library = try? device.makeLibrary(source: Self.kernels, options: nil),
              let cf = library.makeFunction(name: "convert"), let ef = library.makeFunction(name: "estimate"),
              let xf = library.makeFunction(name: "expand"),
              let convert = try? device.makeComputePipelineState(function: cf),
              let estimate = try? device.makeComputePipelineState(function: ef),
              let expand = try? device.makeComputePipelineState(function: xf) else { return nil }
        self.device = device; self.queue = queue; self.event = event; self.scaler = scaler
        self.color = color; self.previous = previous; self.depth = depth; self.motion = motion; self.coarse = coarse
        self.convert = convert; self.estimate = estimate; self.expand = expand
        scaler.colorTexture = color; scaler.depthTexture = depth; scaler.motionTexture = motion
        scaler.outputTexture = output; scaler.inputContentWidth = width; scaler.inputContentHeight = height
        scaler.motionVectorScaleX = 1; scaler.motionVectorScaleY = 1; scaler.preExposure = 1
    }

    func encode(source: MTLTexture, sceneDepth: MTLTexture, jitter: SIMD2<Float>, reversed: Bool, serial: UInt64, reset: Bool) -> Bool {
        stateLock.lock(); let unavailable = failed; stateLock.unlock()
        if unavailable { return false }
        guard source.width == color.width, source.height == color.height,
              sceneDepth.width == color.width, sceneDepth.height == color.height, sceneDepth.sampleCount == 1,
              let cb = queue.makeCommandBuffer() else { return false }
        let now = ProcessInfo.processInfo.systemUptime
        let invalid = reset || !history || now - lastTime > 0.25
        lastTime = now
        cb.label = "Muguet MetalFX Temporal"
        cb.encodeWaitForEvent(event, value: serial * 2 - 1)
        func dispatch(_ pipeline: MTLComputePipelineState, _ textures: [MTLTexture], _ size: MTLSize, _ values: SIMD4<Float>) -> Bool {
            guard let e = cb.makeComputeCommandEncoder() else { return false }
            e.setComputePipelineState(pipeline)
            for (i, texture) in textures.enumerated() { e.setTexture(texture, index: i) }
            var data = values
            e.setBytes(&data, length: MemoryLayout<SIMD4<Float>>.size, index: 0)
            e.dispatchThreads(size, threadsPerThreadgroup: MTLSize(width: 8, height: 8, depth: 1))
            e.endEncoding(); return true
        }
        let full = MTLSize(width: color.width, height: color.height, depth: 1)
        let small = MTLSize(width: coarse.width, height: coarse.height, depth: 1)
        guard dispatch(convert, [source, sceneDepth, color, depth], full, .zero),
              dispatch(estimate, [color, previous, coarse], small, SIMD4<Float>(invalid ? 1 : 0, 0, 0, 0)),
              dispatch(expand, [coarse, motion], full, SIMD4<Float>(jitter.x - lastJitter.x, jitter.y - lastJitter.y, invalid ? 1 : 0, 0)) else { return false }
        scaler.jitterOffsetX = jitter.x; scaler.jitterOffsetY = jitter.y
        scaler.isDepthReversed = reversed; scaler.reset = invalid
        scaler.encode(commandBuffer: cb)
        guard let copy = cb.makeBlitCommandEncoder() else { return false }
        copy.copy(from: color, sourceSlice: 0, sourceLevel: 0, sourceOrigin: .init(x: 0, y: 0, z: 0), sourceSize: .init(width: color.width, height: color.height, depth: 1), to: previous, destinationSlice: 0, destinationLevel: 0, destinationOrigin: .init(x: 0, y: 0, z: 0))
        copy.endEncoding()
        cb.encodeSignalEvent(event, value: serial * 2)
        // A GPU error must not strand the presentation queue waiting on an event.
        cb.addCompletedHandler { [self] completed in
            if completed.status == .error {
                stateLock.lock(); failed = true; stateLock.unlock()
                event.signaledValue = max(event.signaledValue, serial * 2)
                NSLog("Muguet MetalFX failed: %@", completed.error?.localizedDescription ?? "GPU error")
            }
        }
        cb.commit()
        history = true; lastJitter = jitter
        return true
    }

    static let kernels = """
    #include <metal_stdlib>
    using namespace metal;
    kernel void convert(texture2d<float, access::read> src [[texture(0)]], depth2d<float, access::read> ds [[texture(1)]], texture2d<float, access::write> color [[texture(2)]], texture2d<float, access::write> depth [[texture(3)]], uint2 p [[thread_position_in_grid]]) {
        if (p.x >= color.get_width() || p.y >= color.get_height()) return;
        color.write(src.read(p), p); depth.write(float4(ds.read(p)), p);
    }
    float luminance(float3 c) { return dot(c, float3(0.299, 0.587, 0.114)); }
    float error(texture2d<float, access::read> a, texture2d<float, access::read> b, int2 p, int2 offset) {
        const int2 hi = int2(a.get_width()-1, a.get_height()-1);
        float e = 0;
        for (int i=0; i<5; ++i) {
            int2 d = i==0 ? int2(0) : i==1 ? int2(-3,0) : i==2 ? int2(3,0) : i==3 ? int2(0,-3) : int2(0,3);
            e += abs(luminance(a.read(uint2(clamp(p+d,int2(0),hi))).rgb) - luminance(b.read(uint2(clamp(p+d+offset,int2(0),hi))).rgb));
        }
        return e;
    }
    kernel void estimate(texture2d<float, access::read> current [[texture(0)]], texture2d<float, access::read> previous [[texture(1)]], texture2d<float, access::write> coarse [[texture(2)]], constant float4 &params [[buffer(0)]], uint2 p [[thread_position_in_grid]]) {
        if (p.x >= coarse.get_width() || p.y >= coarse.get_height()) return;
        if (params.x > 0) { coarse.write(float4(0), p); return; }
        int2 at = min(int2(p)*8+4, int2(current.get_width()-1, current.get_height()-1));
        int2 best(0); float score = error(current,previous,at,best);
        for(int y=-24;y<=24;y+=4) for(int x=-24;x<=24;x+=4) {
            int2 d(x,y); float v = error(current,previous,at,d) + length(float2(d))*0.0002;
            if(v < score) { score=v; best=d; }
        }
        int2 center=best;
        for(int y=-4;y<=4;y+=2) for(int x=-4;x<=4;x+=2) {
            int2 d=center+int2(x,y); float v=error(current,previous,at,d)+length(float2(d))*0.0002;
            if(v < score) { score=v; best=d; }
        }
        coarse.write(float4(float2(best),0,0),p);
    }
    kernel void expand(texture2d<float, access::sample> coarse [[texture(0)]], texture2d<float, access::write> motion [[texture(1)]], constant float4 &params [[buffer(0)]], uint2 p [[thread_position_in_grid]]) {
        if (p.x >= motion.get_width() || p.y >= motion.get_height()) return;
        constexpr sampler s(coord::normalized, address::clamp_to_edge, filter::linear);
        float2 uv=(float2(p)+0.5)/float2(motion.get_width(),motion.get_height());
        float2 v=params.z>0 ? float2(0) : coarse.sample(s,uv).xy+params.xy;
        motion.write(float4(v,0,0),p);
    }
    """
}

@_cdecl("muguet_temporal_create")
func temporalCreate(_ devicePtr: UnsafeMutableRawPointer, _ outputPtr: UnsafeMutableRawPointer, _ width: UInt32, _ height: UInt32) -> UnsafeMutableRawPointer? {
    if #available(iOS 16.0, macOS 13.0, *) {
        let device = Unmanaged<AnyObject>.fromOpaque(devicePtr).takeUnretainedValue() as! MTLDevice
        let output = Unmanaged<AnyObject>.fromOpaque(outputPtr).takeUnretainedValue() as! MTLTexture
        guard let context = TemporalUpscaler(device: device, output: output, width: Int(width), height: Int(height)) else { return nil }
        return Unmanaged.passRetained(context).toOpaque()
    }
    return nil
}
@_cdecl("muguet_temporal_event")
func temporalEvent(_ ptr: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer? {
    if #available(iOS 16.0, macOS 13.0, *) {
        return Unmanaged.passUnretained(Unmanaged<TemporalUpscaler>.fromOpaque(ptr).takeUnretainedValue().event as AnyObject).toOpaque()
    }
    return nil
}
@_cdecl("muguet_temporal_encode")
func temporalEncode(_ ptr: UnsafeMutableRawPointer, _ colorPtr: UnsafeMutableRawPointer, _ depthPtr: UnsafeMutableRawPointer, _ x: Float, _ y: Float, _ reversed: Bool, _ serial: UInt64, _ reset: Bool) -> Bool {
    if #available(iOS 16.0, macOS 13.0, *) {
        let context = Unmanaged<TemporalUpscaler>.fromOpaque(ptr).takeUnretainedValue()
        let color = Unmanaged<AnyObject>.fromOpaque(colorPtr).takeUnretainedValue() as! MTLTexture
        let depth = Unmanaged<AnyObject>.fromOpaque(depthPtr).takeUnretainedValue() as! MTLTexture
        return context.encode(source: color, sceneDepth: depth, jitter: SIMD2<Float>(x,y), reversed: reversed, serial: serial, reset: reset)
    }
    return false
}
@_cdecl("muguet_temporal_destroy")
func temporalDestroy(_ ptr: UnsafeMutableRawPointer) {
    if #available(iOS 16.0, macOS 13.0, *) { Unmanaged<TemporalUpscaler>.fromOpaque(ptr).release() }
}
