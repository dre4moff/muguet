// MetalFX interop is isolated from the normal presentation path.
use regex::Regex;
use wgpu::*;

// Reconstruct from the selected render size; never reduce the game resolution.
pub fn output_size(input: (u32, u32), ratio: f64, limit: u32) -> Option<(u32, u32)> {
    if ![1.5, 2.0].contains(&ratio) || input.0 == 0 || input.1 == 0 { return None; }
    let scaled = (input.0 as f64 * ratio, input.1 as f64 * ratio);
    if scaled.0 > limit as f64 || scaled.1 > limit as f64 { return None; }
    Some((scaled.0.round() as u32, scaled.1.round() as u32))
}

pub fn jitter(frame: u64) -> [f32; 2] {
    fn halton(mut n: u64, base: u64) -> f32 {
        let (mut value, mut weight) = (0.0, 1.0);
        while n > 0 { weight /= base as f32; value += weight * (n % base) as f32; n /= base; }
        value - 0.5
    }
    let n = frame % 8 + 1;
    [halton(n, 2), halton(n, 3)]
}

// Modify only returns from the vertex entry point; helper functions remain intact.
pub fn inject_jitter(text: &str) -> Option<String> {
    let entry = Regex::new(r"@vertex\s*fn\s+main\s*\([\s\S]*?\)\s*->\s*(?:@builtin\(position\)\s*(vec4<f32>|vec4f)|(\w+))\s*\{").ok()?;
    let hit = entry.captures(text)?;
    let result = hit.get(1).or_else(|| hit.get(2))?.as_str();
    let position = if hit.get(1).is_some() { "output".to_owned() } else {
        let structure = Regex::new(&format!(r"struct\s+{}\s*\{{([^}}]*)\}}", regex::escape(result))).ok()?;
        let body = structure.captures(text)?;
        let member = Regex::new(r"@builtin\(position\)\s+(?:@invariant\s+)?(\w+)\s*:").ok()?.captures(&body[1])?[1].to_owned();
        format!("output.{member}")
    };
    let begin = hit.get(0)?.end();
    let mut braces = 1;
    let mut end = None;
    for (offset, ch) in text[begin..].char_indices() {
        if ch == '{' { braces += 1; }
        if ch == '}' { braces -= 1; if braces == 0 { end = Some(begin + offset); break; } }
    }
    let end = end?;
    let returns = Regex::new(r"return\s+([^;]+);").ok()?;
    if !returns.is_match(&text[begin..end]) { return None; }
    let changed = returns.replace_all(&text[begin..end], "return muguet_jitter_result($1);");
    Some(format!("{}{}{}\n@group(3) @binding(0) var<uniform> muguet_jitter: vec4<f32>;\nfn muguet_jitter_result(value: {result}) -> {result} {{ var output = value; {position}.x += muguet_jitter.x * {position}.w; {position}.y += muguet_jitter.y * {position}.w; return output; }}\n", &text[..begin], changed, &text[end..]))
}

pub struct Temporal {
    pub layout: BindGroupLayout,
    pub groups: Vec<BindGroup>,
    buffers: Vec<Buffer>,
    pub zero: BindGroup,
    pub output: Texture,
    pub input_size: (u32, u32),
    pub output_size: (u32, u32),
    pub failed: bool,
    pub depth: Option<(u32, bool)>,
    serial: u64,
    last_depth: Option<u32>,
    frame: u64,
    written_frame: Option<u64>,
    #[cfg(target_os = "ios")]
    native: *mut std::ffi::c_void,
    #[cfg(target_os = "ios")]
    event: objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLSharedEvent>>,
}

impl Temporal {
    #[cfg(not(target_os = "ios"))]
    pub fn new(_: &Device, _: &Queue, _: (u32,u32), _: (u32,u32), _: usize) -> Option<Self> { None }

    #[cfg(target_os = "ios")]
    pub fn new(device: &Device, queue: &Queue, input_size: (u32,u32), output_size: (u32,u32), slots: usize) -> Option<Self> {
        use objc2::rc::Retained;
        let output = device.create_texture(&TextureDescriptor {
            label: Some("MetalFX output"), size: Extent3d { width: output_size.0, height: output_size.1, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2, format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING | TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let native = unsafe {
            let d = device.as_hal::<hal::api::Metal>()?;
            let t = output.as_hal::<hal::api::Metal>()?;
            muguet_temporal_create(Retained::as_ptr(d.raw_device()) as *mut _, t.raw_handle() as *const _ as *mut _, input_size.0, input_size.1)
        };
        if native.is_null() { return None; }
        let event = unsafe { Retained::retain(muguet_temporal_event(native).cast()) }?;
        if unsafe { queue.as_hal::<hal::api::Metal>()?.enable_strict_event_sync() }.is_err() {
            unsafe { muguet_temporal_destroy(native); } return None;
        }
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor { label: Some("scene jitter"), entries: &[BindGroupLayoutEntry {
            binding: 0, visibility: ShaderStages::VERTEX, ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None,
        }] });
        let mut groups = Vec::new(); let mut buffers = Vec::new();
        for _ in 0..slots+1 {
            let b = device.create_buffer(&BufferDescriptor { label: Some("scene jitter"), size: 16, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false });
            queue.write_buffer(&b, 0, &[0;16]);
            groups.push(device.create_bind_group(&BindGroupDescriptor { label: None, layout: &layout, entries: &[BindGroupEntry { binding: 0, resource: b.as_entire_binding() }] }));
            buffers.push(b);
        }
        let zero = groups.pop().unwrap();
        Some(Self { layout, groups, buffers, zero, output, input_size, output_size, failed: false, depth: None, serial: 0, last_depth: None, frame: 0, written_frame: None, native, event })
    }

    pub fn scene_group(&mut self, queue: &Queue, slot: usize, depth: Option<(u32,bool)>) -> BindGroup {
        if self.failed || depth.is_none() { return self.zero.clone(); }
        self.depth = depth;
        if self.written_frame != Some(self.frame) {
            let j = jitter(self.frame);
            let values = [2.0*j[0]/self.input_size.0 as f32, -2.0*j[1]/self.input_size.1 as f32, 0.0, 0.0];
            let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            queue.write_buffer(&self.buffers[slot], 0, &bytes);
            self.written_frame = Some(self.frame);
        }
        self.groups[slot].clone()
    }

    #[cfg(target_os = "ios")]
    pub fn prepare(&mut self, queue: &Queue) {
        self.serial += 1;
        unsafe { queue.as_hal::<hal::api::Metal>().unwrap().add_signal_event(self.event.clone(), self.serial*2-1); }
    }
    #[cfg(not(target_os = "ios"))]
    pub fn prepare(&mut self, _: &Queue) {}

    #[cfg(target_os = "ios")]
    pub fn encode(&mut self, queue: &Queue, color: &Texture, depth: &Texture, id: u32, reversed: bool) -> bool {
        let j = jitter(self.frame);
        let ok = unsafe {
            let c = color.as_hal::<hal::api::Metal>().unwrap();
            let d = depth.as_hal::<hal::api::Metal>().unwrap();
            muguet_temporal_encode(self.native, c.raw_handle() as *const _ as *mut _, d.raw_handle() as *const _ as *mut _, j[0], j[1], reversed, self.serial, self.last_depth != Some(id))
        };
        if ok {
            unsafe { queue.as_hal::<hal::api::Metal>().unwrap().add_wait_event(self.event.clone(), self.serial*2); }
            self.last_depth = Some(id);
        } else { self.failed = true; }
        ok
    }
    #[cfg(not(target_os = "ios"))]
    pub fn encode(&mut self, _: &Queue, _: &Texture, _: &Texture, _: u32, _: bool) -> bool { false }

    pub fn last_active(&self) -> bool { self.last_depth.is_some() && self.depth.is_some() }
    pub fn next_frame(&mut self) {
        if self.depth.is_none() { self.last_depth = None; }
        self.frame += 1; self.depth = None;
    }
}

#[cfg(target_os = "ios")]
impl Drop for Temporal { fn drop(&mut self) { unsafe { muguet_temporal_destroy(self.native); } } }

#[cfg(target_os = "ios")]
extern "C" {
    fn muguet_temporal_create(device: *mut std::ffi::c_void, output: *mut std::ffi::c_void, w:u32, h:u32) -> *mut std::ffi::c_void;
    fn muguet_temporal_event(ptr: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn muguet_temporal_encode(ptr: *mut std::ffi::c_void, color: *mut std::ffi::c_void, depth: *mut std::ffi::c_void, x:f32,y:f32,reversed:bool,serial:u64,reset:bool) -> bool;
    fn muguet_temporal_destroy(ptr: *mut std::ffi::c_void);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn ratios_reconstruct_the_selected_input_without_display_scaling() {
        assert_eq!(output_size((1280, 720), 1.5, 8192), Some((1920, 1080)));
        assert_eq!(output_size((1280, 720), 2.0, 8192), Some((2560, 1440)));
        assert_eq!(output_size((2560, 1440), 2.0, 8192), Some((5120, 2880)));
        assert_eq!(output_size((1202, 676), 1.5, 8192), Some((1803, 1014)));
        assert_eq!(output_size((2560, 1440), 2.0, 4096), None);
        assert_eq!(output_size((0, 720), 1.5, 8192), None);
        assert_eq!(output_size((1280, 720), f64::NAN, 8192), None);
        assert_eq!(output_size((1280, 720), 3.0, 8192), None);
    }
    #[test] fn entry_returns_are_jittered_without_changing_helpers() {
        let source = "struct Out { @builtin(position) position: vec4<f32>, }; fn helper()->f32 { return 1.0; } @vertex fn main(@builtin(vertex_index) i:u32)->Out { return Out(vec4<f32>(0.0,0.0,0.0,1.0)); }";
        let changed = inject_jitter(source).unwrap();
        assert!(changed.contains("fn helper()->f32 { return 1.0; }"));
        assert!(changed.contains("return muguet_jitter_result(Out("));
        assert_eq!(jitter(0), jitter(8));
        assert!(jitter(1).iter().all(|v| v.abs() <= 0.5));
    }
    #[test] fn jittered_vertex_programs_pass_gpu_validation() {
        let mut descriptor = InstanceDescriptor::new_without_display_handle();
        descriptor.backends = Backends::METAL;
        let instance = Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let features = Features::CLIP_DISTANCES | Features::DUAL_SOURCE_BLENDING | Features::DEPTH32FLOAT_STENCIL8 | Features::FLOAT32_FILTERABLE | Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor { required_features: adapter.features() & features, ..Default::default() })).unwrap();
        let scope = device.push_error_scope(ErrorFilter::Validation);
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor { label: None, entries: &[BindGroupLayoutEntry { binding: 0, visibility: ShaderStages::VERTEX, ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor { label: None, bind_group_layouts: &[None,None,None,Some(&layout)], immediate_size: 0 });
        let source = "struct Out { @builtin(position) position: vec4<f32>, }; @vertex fn main(@builtin(vertex_index) i:u32)->Out { return Out(vec4<f32>(0.0,0.0,0.0,1.0)); } @fragment fn fs()->@location(0) vec4<f32> { return vec4<f32>(1.0); }";
        let module = device.create_shader_module(ShaderModuleDescriptor { label: None, source: ShaderSource::Wgsl(inject_jitter(source).unwrap().into()) });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor { label: None, layout: Some(&pipeline_layout), vertex: VertexState { module: &module, entry_point: Some("main"), compilation_options: Default::default(), buffers: &[] }, fragment: Some(FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(ColorTargetState { format: TextureFormat::Rgba8Unorm, blend: None, write_mask: ColorWrites::ALL })] }), primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None });
        let buffer = device.create_buffer(&BufferDescriptor { label: None, size: 16, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false });
        queue.write_buffer(&buffer, 0, &[0;16]);
        let group = device.create_bind_group(&BindGroupDescriptor { label: None, layout: &layout, entries: &[BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }] });
        let target = device.create_texture(&TextureDescriptor { label: None, size: Extent3d { width: 16,height:16,depth_or_array_layers:1 }, mip_level_count:1,sample_count:1,dimension:TextureDimension::D2,format:TextureFormat::Rgba8Unorm,usage:TextureUsages::RENDER_ATTACHMENT,view_formats:&[] });
        let view = target.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor { label: None, color_attachments:&[Some(RenderPassColorAttachment { view:&view,depth_slice:None,resolve_target:None,ops:Operations { load:LoadOp::Clear(Color::BLACK),store:StoreOp::Store } })],depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None,multiview_mask:None });
            pass.set_pipeline(&pipeline); pass.set_bind_group(3,&group,&[]); pass.draw(0..3,0..1);
        }
        let submission = queue.submit([encoder.finish()]);
        device.poll(PollType::Wait { submission_index:Some(submission),timeout:None }).unwrap();
        assert!(pollster::block_on(scope.pop()).is_none(), "Jitter bind-group layout failed a real GPU draw");
        let mut programs = vec!["struct Out { @builtin(position) position: vec4<f32>, }; @vertex fn main(@builtin(vertex_index) i:u32)->Out { return Out(vec4<f32>(0.0,0.0,0.0,1.0)); }".to_owned()];
        if let Ok(path) = std::env::var("MUGUET_TEST_GAME_ZIP") {
            let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
            use std::io::Read;
            for n in 0..archive.len() {
                let mut entry = archive.by_index(n).unwrap();
                if !entry.name().ends_with(".wgsl") { continue; }
                let mut text = String::new(); entry.read_to_string(&mut text).unwrap();
                if text.contains("@vertex") { programs.push(text); }
            }
        }
        let total = programs.len();
        for (i, source) in programs.into_iter().enumerate() {
            let text = inject_jitter(&source).expect("Unsupported vertex output shape");
            let scope = device.push_error_scope(ErrorFilter::Validation);
            let _ = device.create_shader_module(ShaderModuleDescriptor { label: Some("jitter validation"), source: ShaderSource::Wgsl(text.into()) });
            assert!(pollster::block_on(scope.pop()).is_none(), "Vertex shader {i} failed jitter validation");
        }
        eprintln!("Validated jitter injection for {total} vertex programs");
    }
}
