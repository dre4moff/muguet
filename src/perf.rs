// Muguet fork — opt-in renderer diagnostics, GPL-3.0-or-later.
// Wall waits are not GPU execution. CPU clocks count actual CPU time.
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, atomic::{AtomicU8, Ordering}};
use std::time::Instant;
use wgpu::*;

pub fn enabled() -> bool { std::env::var("MUGUET_PERFORMANCE_HUD").as_deref() == Ok("1") }
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
    pub submit: Cell<u64>, pub uniform: Cell<u64>, pub uniform_bytes: Cell<u64>,
    pub readbacks: Cell<u64>, pub fences: Cell<u64>, pub groups: Cell<u64>,
}
pub fn add(cell: &Cell<u64>, start: Option<Instant>) {
    if let Some(start) = start { cell.set(cell.get().saturating_add(start.elapsed().as_nanos() as u64)); }
}
pub struct Monitor {
    pub enabled: bool, pub costs: Costs,
    previous: [u64; 12],
}
impl Monitor {
    pub fn new(enabled: bool) -> Self {
        let mut result = Self { enabled, costs: Costs::default(), previous: [0; 12] };
        if enabled { result.previous[0] = cpu_ns(true); result.previous[1] = cpu_ns(false); }
        result
    }
    pub fn start(&self) -> Option<Instant> { self.enabled.then(Instant::now) }
    pub fn report(&mut self, frames: u64, seconds: f64) -> String {
        let c = &self.costs;
        let now = [cpu_ns(true), cpu_ns(false), c.idle.get(), c.fence.get(), c.surface.get(), c.submit.get(), c.uniform.get(), c.uniform_bytes.get(), c.readbacks.get(), c.fences.get(), c.groups.get(), crate::io::READ_NS.load(Ordering::Relaxed)];
        let delta: Vec<_> = now.iter().zip(self.previous).map(|(a,b)| a.saturating_sub(b)).collect();
        self.previous = now;
        let ms = |i: usize| delta[i] as f64 / 1e6 / frames.max(1) as f64;
        let cores = delta[1] as f64 / (seconds.max(0.001) * 1e9);
        format!("Renderer CPU {:.1} ms/f · app CPU {:.2} cores\nCmd idle {:.1} · fence {:.1} · drawable {:.1} ms/f\nSubmit {:.1} · uniforms {:.1} ms/f · {:.2} MiB/f\nReadback {} · fences {} · new groups {} · I/O {:.1} ms/f", ms(0), cores, ms(2), ms(3), ms(4), ms(5), ms(6), delta[7] as f64 / 1048576.0 / frames.max(1) as f64, delta[8], delta[9], delta[10], ms(11))
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
        let sum = self.samples.iter().map(|s| s.0).sum::<f64>() / n;
        let span = self.samples.iter().map(|s| s.1).sum::<f64>() / n;
        let partial = self.samples.iter().any(|s| s.3 > 0);
        format!("GPU pass sum {sum:.1} · span {span:.1} ms/f{}\n{} samples · age {:.1}s · errors {}", if partial { " (partial)" } else { "" }, self.samples.len(), self.last_result.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0), self.failures)
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
