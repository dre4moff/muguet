// Muguet fork — opt-in renderer diagnostics, GPL-3.0-or-later.
// Wall waits are not GPU execution. CPU clocks count actual CPU time.
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, atomic::{AtomicU8, Ordering}};
use std::time::Instant;
use wgpu::*;

pub fn enabled() -> bool { std::env::var("MUGUET_PERFORMANCE_HUD").as_deref() == Ok("1") }
pub fn gpu_enabled() -> bool { std::env::var("MUGUET_GPU_PROFILING").as_deref() == Ok("1") }
pub fn cpu_ns(thread: bool) -> u64 {
    #[repr(C)] struct Timespec { sec: i64, nsec: i64 }
    extern "C" { fn clock_gettime(clock: u32, time: *mut Timespec) -> i32; }
    let mut t = Timespec { sec: 0, nsec: 0 };
    // Darwin SDK CLOCK_THREAD_CPUTIME_ID / CLOCK_PROCESS_CPUTIME_ID.
    if unsafe { clock_gettime(if thread { 16 } else { 12 }, &mut t) } != 0 { return 0; }
    (t.sec as u64).saturating_mul(1_000_000_000).saturating_add(t.nsec as u64)
}
#[derive(Default)]
pub struct Costs {
    pub idle: Cell<u64>, pub fence: Cell<u64>, pub surface: Cell<u64>,
    pub encode: Cell<u64>, pub submit: Cell<u64>, pub uniform: Cell<u64>, pub uniform_bytes: Cell<u64>,
    pub readbacks: Cell<u64>, pub fences: Cell<u64>, pub groups: Cell<u64>,
}
pub fn add(cell: &Cell<u64>, start: Option<Instant>) {
    if let Some(start) = start { cell.set(cell.get().saturating_add(start.elapsed().as_nanos() as u64)); }
}
pub struct Monitor {
    pub enabled: bool, pub costs: Costs,
    previous: [u64; 13],
}
impl Monitor {
    pub fn new(enabled: bool) -> Self {
        let mut result = Self { enabled, costs: Costs::default(), previous: [0; 13] };
        if enabled { result.previous[0] = cpu_ns(true); result.previous[1] = cpu_ns(false); }
        result
    }
    pub fn start(&self) -> Option<Instant> { self.enabled.then(Instant::now) }
    pub fn report(&mut self, frames: u64, seconds: f64) -> String {
        let c = &self.costs;
        let now = [cpu_ns(true), cpu_ns(false), c.idle.get(), c.fence.get(), c.surface.get(), c.encode.get(), c.submit.get(), c.uniform.get(), c.uniform_bytes.get(), c.readbacks.get(), c.fences.get(), c.groups.get(), crate::io::READ_NS.load(Ordering::Relaxed)];
        let delta: Vec<_> = now.iter().zip(self.previous).map(|(a,b)| a.saturating_sub(b)).collect();
        self.previous = now;
        let ms = |i: usize| delta[i] as f64 / 1e6 / frames.max(1) as f64;
        let cores = delta[1] as f64 / (seconds.max(0.001) * 1e9);
        format!("Renderer CPU {:.1} ms/f · app CPU {:.2} cores\nCmd idle {:.1} · fence {:.1} · drawable {:.1} ms/f\nMetal encode {:.1} · queue {:.1} · uniforms {:.1} ms/f · {:.2} MiB/f\nReadback {} · fences {} · new groups {} · I/O {:.1} ms/f", ms(0), cores, ms(2), ms(3), ms(4), ms(5), ms(6), ms(7), delta[8] as f64 / 1048576.0 / frames.max(1) as f64, delta[9], delta[10], delta[11], ms(12))
    }
}

const QUERIES: u32 = 2048;
const SAMPLE_INTERVAL: u64 = 16;
struct Slot {
    queries: QuerySet, resolve: Buffer, read: Buffer,
    // 0 free, 1 submitted/unmapped, 2 successful map, 3 failed map, 4 ready to map, 5 disabled.
    state: Arc<AtomicU8>, used: u32, dropped: u32,
}
pub struct PassTiming { queries: QuerySet, index: u32 }
impl PassTiming {
    pub fn render(&self) -> RenderPassTimestampWrites<'_> {
        RenderPassTimestampWrites { query_set: &self.queries, beginning_of_pass_write_index: Some(self.index), end_of_pass_write_index: Some(self.index + 1) }
    }
    pub fn compute(&self) -> ComputePassTimestampWrites<'_> {
        ComputePassTimestampWrites { query_set: &self.queries, beginning_of_pass_write_index: Some(self.index), end_of_pass_write_index: Some(self.index + 1) }
    }
}
pub struct GpuTiming {
    slots: Vec<Slot>, active: Option<usize>,
    // Samples are per-frame pass sums and envelopes; neither is GPU utilization.
    samples: VecDeque<(f64, f64, u32, u32)>, last_result: Option<Instant>, failures: u64,
}
impl GpuTiming {
    pub fn new(device: &Device) -> Option<Self> {
        if !device.features().contains(Features::TIMESTAMP_QUERY) { return None; }
        let slots = (0..3).map(|_| Slot {
            queries: device.create_query_set(&QuerySetDescriptor { label: Some("HUD pass timestamps"), ty: QueryType::Timestamp, count: QUERIES }),
            resolve: device.create_buffer(&BufferDescriptor { label: Some("HUD query resolve"), size: QUERIES as u64 * 8, usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC, mapped_at_creation: false }),
            read: device.create_buffer(&BufferDescriptor { label: Some("HUD asynchronous timestamps"), size: QUERIES as u64 * 8, usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ, mapped_at_creation: false }),
            state: Arc::new(AtomicU8::new(0)), used: 0, dropped: 0,
        }).collect();
        let mut result = Self { slots, active: None, samples: VecDeque::new(), last_result: None, failures: 0 };
        result.start_frame(0);
        Some(result)
    }
    pub fn start_frame(&mut self, frame: u64) {
        self.active = None;
        if frame % SAMPLE_INTERVAL != 0 { return; }
        if let Some(i) = self.slots.iter().position(|s| s.state.load(Ordering::Acquire) == 0) {
            self.slots[i].used = 0; self.slots[i].dropped = 0; self.active = Some(i);
        }
    }
    pub fn next(&mut self) -> Option<PassTiming> {
        let s = &mut self.slots[self.active?];
        if s.used + 2 > QUERIES { s.dropped += 1; return None; }
        let index = s.used; s.used += 2;
        Some(PassTiming { queries: s.queries.clone(), index })
    }
    pub fn needs_finish(&self) -> bool { self.active.map(|i| self.slots[i].used > 0).unwrap_or(false) }
    pub fn finish(&mut self, encoder: &mut CommandEncoder) {
        let Some(i) = self.active.take() else { return };
        let s = &self.slots[i];
        if s.used == 0 { return; }
        encoder.resolve_query_set(&s.queries, 0..s.used, &s.resolve, 0);
        encoder.copy_buffer_to_buffer(&s.resolve, 0, &s.read, 0, s.used as u64 * 8);
        s.state.store(4, Ordering::Release);
    }
    pub fn arm_maps(&self) {
        for s in &self.slots {
            if s.state.compare_exchange(4, 1, Ordering::AcqRel, Ordering::Acquire).is_err() { continue; }
            let state = s.state.clone();
            s.read.slice(0..s.used as u64 * 8).map_async(MapMode::Read, move |r| state.store(if r.is_ok() { 2 } else { 3 }, Ordering::Release));
        }
    }
    pub fn collect(&mut self, device: &Device) {
        if self.slots.iter().any(|s| s.state.load(Ordering::Acquire) == 1) { let _ = device.poll(PollType::Poll); }
        for s in &self.slots {
            match s.state.load(Ordering::Acquire) {
                2 => {
                    let Ok(range) = s.read.slice(0..s.used as u64 * 8).get_mapped_range() else {
                        self.failures += 1; s.state.store(5, Ordering::Release); continue;
                    };
                    let times: Vec<_> = range.chunks_exact(8).map(|b| u64::from_ne_bytes(b.try_into().unwrap())).collect();
                    if let Some((sum, span, valid, missing)) = durations(&times) {
                        self.samples.push_back((sum, span, valid, s.dropped + missing));
                        if self.samples.len() > 8 { self.samples.pop_front(); }
                        self.last_result = Some(Instant::now());
                    } else { self.failures += 1; }
                    drop(range); s.read.unmap(); s.state.store(0, Ordering::Release);
                }
                3 => { self.failures += 1; s.read.unmap(); s.state.store(5, Ordering::Release); }
                _ => {}
            }
        }
    }
    pub fn report(&self) -> String {
        if self.samples.is_empty() { return format!("GPU pass timestamps: waiting · errors {}", self.failures); }
        let n = self.samples.len() as f64;
        // Overlapping vertex/fragment intervals cannot be added as GPU frame time.
        let span = self.samples.iter().map(|s| s.1).sum::<f64>() / n;
        let partial = self.samples.iter().any(|s| s.3 > 0);
        format!("GPU sampled span {span:.1} ms{}\n{} samples · age {:.1}s · errors {}", if partial { " (partial)" } else { "" }, self.samples.len(), self.last_result.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0), self.failures)
    }
}
fn durations(times: &[u64]) -> Option<(f64, f64, u32, u32)> {
    if times.is_empty() || times.len() % 2 != 0 { return None; }
    let mut sum = 0u64; let mut first = u64::MAX; let mut last = 0;
    let mut valid = 0; let mut missing = 0;
    for pair in times.chunks_exact(2) {
        if pair[1] < pair[0] || pair[0] == 0 { missing += 1; continue; }
        valid += 1;
        sum = sum.checked_add(pair[1] - pair[0])?;
        first = first.min(pair[0]); last = last.max(pair[1]);
    }
    if valid == 0 { return None; }
    Some((sum as f64 / 1e6, (last - first) as f64 / 1e6, valid, missing))
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn timestamps_do_not_count_queue_gaps_as_pass_work() {
        assert_eq!(durations(&[1_000_000, 3_000_000, 8_000_000, 9_000_000]), Some((3.0, 8.0, 2, 0)));
        assert_eq!(durations(&[5, 4]), None); assert_eq!(durations(&[0, 4]), None);
        assert_eq!(durations(&[1_000_000, 3_000_000, 8_000_000, 0]), Some((2.0, 2.0, 1, 1)));
    }
}

#[cfg(test)] mod metal_tests {
    use super::*;
    #[test]
    fn bounded_gpu_pass_sampling_spans_multiple_submissions() {
        let mut descriptor = InstanceDescriptor::new_without_display_handle();
        descriptor.backends = Backends::METAL;
        descriptor.flags.insert(InstanceFlags::AUTOMATIC_TIMESTAMP_NORMALIZATION);
        let instance = Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        if !adapter.features().contains(Features::TIMESTAMP_QUERY) { eprintln!("GPU timestamps unsupported on test host"); return; }
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor { required_features: Features::TIMESTAMP_QUERY, ..Default::default() })).unwrap();
        let scope = device.push_error_scope(ErrorFilter::Validation);
        let mut timing = GpuTiming::new(&device).unwrap();
        let texture = device.create_texture(&TextureDescriptor { label: None, size: Extent3d { width: 64, height: 64, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2, format: TextureFormat::Bgra8Unorm, usage: TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
        let view = texture.create_view(&TextureViewDescriptor::default());
        let shader = device.create_shader_module(ShaderModuleDescriptor { label: None, source: ShaderSource::Wgsl("@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4f { var p=array<vec2f,3>(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3)); return vec4f(p[i],0,1); } @fragment fn fs()->@location(0) vec4f { return vec4f(0.2,0.5,0.8,1); }".into()) });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None, layout: None,
            vertex: VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            fragment: Some(FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(ColorTargetState { format: TextureFormat::Bgra8Unorm, blend: None, write_mask: ColorWrites::ALL })] }),
            multiview_mask: None, cache: None,
        });
        // Saturate all three slots without collecting; the fourth sample must skip.
        for frame in [0,16,32,48] {
            timing.start_frame(frame);
            if frame == 48 { assert!(timing.next().is_none()); continue; }
            for submission in 0..2 {
                let t = timing.next().unwrap();
                let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
                {
                    let mut pass = enc.begin_render_pass(&RenderPassDescriptor {
                        label: None, color_attachments: &[Some(RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Clear(Color::GREEN), store: StoreOp::Store } })],
                        depth_stencil_attachment: None, timestamp_writes: Some(t.render()), occlusion_query_set: None, multiview_mask: None,
                    });
                    pass.set_pipeline(&pipeline); pass.draw(0..3, 0..1);
                }
                if submission == 1 { timing.finish(&mut enc); }
                queue.submit([enc.finish()]); timing.arm_maps();
            }
        }
        // Waiting is permitted only in this host correctness test, not in profiler.
        device.poll(PollType::Wait { submission_index: None, timeout: None }).unwrap();
        timing.collect(&device);
        eprintln!("GPU profiler result: {}; states {:?}", timing.report(), timing.slots.iter().map(|s| s.state.load(Ordering::Acquire)).collect::<Vec<_>>());
        assert!(pollster::block_on(scope.pop()).is_none());
        assert_eq!(timing.samples.len() as u64 + timing.failures, 3); assert!(!timing.samples.is_empty());
        for sample in &timing.samples { assert!(sample.0 >= 0.0 && sample.1 >= 0.0); assert_eq!(sample.2 + sample.3, 2); }
        timing.start_frame(64); assert!(timing.next().is_some());
        eprintln!("GPU timing host validation: {}", timing.report());
    }
}

#[cfg(test)] mod submission_benchmark {
    use super::*;
    #[test] #[ignore = "synthetic Metal CPU cost, not iPhone FPS"]
    fn loaded_resource_submission_cost() {
        let mut desc = InstanceDescriptor::new_without_display_handle(); desc.backends = Backends::METAL;
        let instance = Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
        let errors = device.push_error_scope(ErrorFilter::Validation);
        let mut resources = Vec::new();
        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {label: None, entries: &[BindGroupLayoutEntry { binding: 0, visibility: ShaderStages::VERTEX_FRAGMENT, ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: std::num::NonZeroU64::new(16) }, count: None }]});
        let uniform = device.create_buffer(&BufferDescriptor {label: None, size: 3370*256, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false});
        let bg = device.create_bind_group(&BindGroupDescriptor {label: None, layout: &bgl, entries: &[BindGroupEntry {binding: 0, resource: BindingResource::Buffer(BufferBinding {buffer: &uniform, offset: 0, size: std::num::NonZeroU64::new(16)})}]});
        let texture = device.create_texture(&TextureDescriptor {label: None, size: Extent3d {width: 16, height: 16, depth_or_array_layers: 1}, mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2, format: TextureFormat::Rgba8Unorm, usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC, view_formats: &[]});
        let view = texture.create_view(&TextureViewDescriptor::default());
        let shader = device.create_shader_module(ShaderModuleDescriptor {label: None, source: ShaderSource::Wgsl("@group(0) @binding(0) var<uniform> c:vec4f; @vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4f { var p=array<vec2f,3>(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3)); return vec4f(p[i],0,1); } @fragment fn fs()->@location(0) vec4f { return vec4f(0.2,0.5,0.8,1)+c; }".into())});
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {label: None, bind_group_layouts: &[Some(&bgl)], immediate_size: 0});
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {label: None, layout: Some(&layout), vertex: VertexState {module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[]}, primitive: Default::default(), depth_stencil: None, multisample: Default::default(), fragment: Some(FragmentState {module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(ColorTargetState {format: TextureFormat::Rgba8Unorm, blend: None, write_mask: ColorWrites::ALL})]}), multiview_mask: None, cache: None});
        for total in [0, 8192, 32768, 65536] {
            while resources.len() < total { resources.push(device.create_buffer(&BufferDescriptor {label: None, size: 16, usage: BufferUsages::VERTEX, mapped_at_creation: false})); }
            let mut measurements = Vec::new();
            for frame in 0..64 {
                let begin = Instant::now();
                let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
                for pass_index in 0..65 {
                    let mut pass = enc.begin_render_pass(&RenderPassDescriptor {label: None, color_attachments: &[Some(RenderPassColorAttachment {view: &view, depth_slice: None, resolve_target: None, ops: Operations {load: LoadOp::Clear(Color::BLACK), store: StoreOp::Store}})], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None});
                    pass.set_pipeline(&pipeline);
                    for draw in 0..52 { if total > 0 { pass.set_vertex_buffer(0, resources[((pass_index*997+draw*173) as usize)%total].slice(..)); } pass.set_bind_group(0, &bg, &[((pass_index*52+draw)%3370)*256]); pass.draw(0..3, 0..1); }
                }
                let recorded = begin.elapsed().as_secs_f64()*1000.;
                let t = Instant::now(); let commands = enc.finish(); let finish = t.elapsed().as_secs_f64()*1000.;
                let t = Instant::now(); let index = queue.submit([commands]); let submit = t.elapsed().as_secs_f64()*1000.;
                device.poll(PollType::Wait {submission_index: Some(index), timeout: None}).unwrap();
                if frame >= 8 { measurements.push((recorded, finish, submit)); }
            }
            let median = |get: fn(&(f64,f64,f64))->f64| { let mut v:Vec<_>=measurements.iter().map(get).collect(); v.sort_by(f64::total_cmp); (v[v.len()/2-1]+v[v.len()/2])/2. };
            eprintln!("SUBMIT_BENCH buffers={total} record_ms={:.3} encode_ms={:.3} queue_ms={:.3}", median(|x|x.0), median(|x|x.1), median(|x|x.2));
        }
        assert!(pollster::block_on(errors.pop()).is_none());
    }
}

#[cfg(test)] mod lifetime_tests {
    use super::*;
    #[test] fn tracker_reuse_preserves_inflight_pixels_and_destroy_validation() {
        let mut desc = InstanceDescriptor::new_without_display_handle(); desc.backends = Backends::METAL;
        let instance = Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
        let scope = device.push_error_scope(ErrorFilter::Validation);
        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {label: None, entries: &[BindGroupLayoutEntry {binding: 0, visibility: ShaderStages::FRAGMENT, ty: BindingType::Buffer {ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: std::num::NonZeroU64::new(16)}, count: None}]});
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {label: None, bind_group_layouts: &[Some(&bgl)], immediate_size: 0});
        let shader = device.create_shader_module(ShaderModuleDescriptor {label: None, source: ShaderSource::Wgsl("@group(0) @binding(0) var<uniform> color:vec4f; @vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4f { var p=array<vec2f,3>(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3)); return vec4f(p[i],0,1); } @fragment fn fs()->@location(0) vec4f { return color; }".into())});
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {label: None, layout: Some(&layout), vertex: VertexState {module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[]}, primitive: Default::default(), depth_stencil: None, multisample: Default::default(), fragment: Some(FragmentState {module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(ColorTargetState {format: TextureFormat::Rgba8Unorm, blend: None, write_mask: ColorWrites::ALL})]}), multiview_mask: None, cache: None});
        let texture = device.create_texture(&TextureDescriptor {label: None, size: Extent3d {width: 1, height: 1, depth_or_array_layers: 1}, mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2, format: TextureFormat::Rgba8Unorm, usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC, view_formats: &[]});
        let view = texture.create_view(&TextureViewDescriptor::default());
        let make_group = |color: [f32;4]| {
            let buf = device.create_buffer(&BufferDescriptor {label: None, size: 16, usage: BufferUsages::UNIFORM, mapped_at_creation: true});
            { let mut mapped = buf.slice(..).get_mapped_range_mut().unwrap(); let mut bytes=[0u8;16]; for (dest,value) in bytes.chunks_exact_mut(4).zip(color) { dest.copy_from_slice(&value.to_ne_bytes()); } mapped.copy_from_slice(&bytes); }
            buf.unmap();
            let bg = device.create_bind_group(&BindGroupDescriptor {label: None, layout: &bgl, entries: &[BindGroupEntry {binding: 0, resource: buf.as_entire_binding()}]}); (buf,bg)
        };
        let record = |enc: &mut CommandEncoder, groups: &[&BindGroup]| {
            let mut pass = enc.begin_render_pass(&RenderPassDescriptor {label: None, color_attachments: &[Some(RenderPassColorAttachment {view: &view, depth_slice: None, resolve_target: None, ops: Operations {load: LoadOp::Clear(Color::BLACK), store: StoreOp::Store}})], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None});
            pass.set_pipeline(&pipeline);
            for bg in groups { pass.set_bind_group(0, *bg, &[]); }
            pass.draw(0..3, 0..1);
        };
        let mut reads = Vec::new();
        // Keep several submissions in flight; drop user owners before finish,
        // then retire/reuse the pool between batches with differing colors.
        for frame in 0..32u32 {
            let (buf,bg) = make_group([frame as f32/255., 0., 1., 1.]);
            let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
            record(&mut enc, &[&bg]); drop(bg); drop(buf);
            let read = device.create_buffer(&BufferDescriptor {label: None, size: 256, usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST, mapped_at_creation: false});
            enc.copy_texture_to_buffer(TexelCopyTextureInfo {texture: &texture, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All}, TexelCopyBufferInfo {buffer: &read, layout: TexelCopyBufferLayout {offset: 0, bytes_per_row: Some(256), rows_per_image: Some(1)}}, Extent3d {width: 1, height: 1, depth_or_array_layers: 1});
            queue.submit([enc.finish()]); reads.push(read);
            if frame%4==3 { device.poll(PollType::Wait {submission_index: None, timeout: None}).unwrap(); }
        }
        for (frame,read) in reads.iter().enumerate() {
            let (tx,rx) = std::sync::mpsc::channel(); read.slice(..).map_async(MapMode::Read, move |r|tx.send(r).unwrap());
            device.poll(PollType::Wait {submission_index: None, timeout: None}).unwrap(); rx.recv().unwrap().unwrap();
            {let mapped=read.slice(..).get_mapped_range().unwrap(); assert_eq!(&mapped[..4], &[frame as u8,0,255,255]);} read.unmap();
        }
        assert!(pollster::block_on(scope.pop()).is_none());
        // Overwritten/unused bindings must still fail if destroyed before submit.
        let scope = device.push_error_scope(ErrorFilter::Validation);
        let (dead,old)=make_group([1.,0.,0.,1.]); let (_live,new)=make_group([0.,1.,0.,1.]);
        let mut enc=device.create_command_encoder(&CommandEncoderDescriptor::default()); record(&mut enc,&[&old,&new]);
        let commands=enc.finish(); dead.destroy(); queue.submit([commands]);
        let error=pollster::block_on(scope.pop()).expect("destroy validation must remain active");
        assert!(error.to_string().contains("destroyed"), "{error}");
        eprintln!("32 in-flight pixel results and destroyed overwritten binding validated");
    }
}
