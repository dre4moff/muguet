use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::ops::{Deref, DerefMut};
use crate::{device::Device, track::Tracker};

use crate::lock::{rank, Mutex};

/// A pool of free [`wgpu_hal::CommandEncoder`]s, owned by a `Device`.
///
/// Each encoder in this list is in the "closed" state.
///
/// Since a raw [`CommandEncoder`][ce] is itself a pool for allocating
/// raw [`CommandBuffer`][cb]s, this is a pool of pools.
///
/// [`wgpu_hal::CommandEncoder`]: hal::CommandEncoder
/// [ce]: hal::CommandEncoder
/// [cb]: hal::Api::CommandBuffer
pub(crate) struct CommandAllocator {
    free_encoders: Mutex<FreeCommands>,
}

impl CommandAllocator {
    pub(crate) fn new() -> Self {
        Self {
            free_encoders: Mutex::new(rank::COMMAND_ALLOCATOR_FREE_ENCODERS, FreeCommands::default()),
        }
    }

    /// Return a fresh [`wgpu_hal::CommandEncoder`] in the "closed" state.
    ///
    /// If we have free encoders in the pool, take one of those. Otherwise,
    /// create a new one on `device`.
    ///
    /// [`wgpu_hal::CommandEncoder`]: hal::CommandEncoder
    pub(crate) fn acquire_encoder(
        &self,
        device: &dyn hal::DynDevice,
        queue: &dyn hal::DynQueue,
    ) -> Result<Box<dyn hal::DynCommandEncoder>, hal::DeviceError> {
        let mut free_encoders = self.free_encoders.lock();
        match free_encoders.encoders.pop() {
            Some(encoder) => Ok(encoder),
            None => unsafe {
                let hal_desc = hal::CommandEncoderDescriptor { label: None, queue };
                device.create_command_encoder(&hal_desc)
            },
        }
    }

    /// Add `encoder` back to the free pool.
    pub(crate) fn release_encoder(&self, encoder: Box<dyn hal::DynCommandEncoder>) {
        let mut free_encoders = self.free_encoders.lock();
        free_encoders.encoders.push(encoder);
    }
}

// Empty CPU tracking tables share the allocator's lock and lifetime. Resources
// are released before pooling; in-flight trackers never enter this pool.
#[derive(Default)]
struct FreeCommands {
    encoders: Vec<Box<dyn hal::DynCommandEncoder>>,
    trackers: Vec<Tracker>,
}
impl CommandAllocator {
    pub(crate) fn acquire_tracker(&self, device: &Arc<Device>) -> PooledTracker {
        let inner = self.free_encoders.lock().trackers.pop().unwrap_or_else(||
            Tracker::new(device.ordered_buffer_usages, device.ordered_texture_usages));
        PooledTracker { inner: Some(inner), device: device.clone() }
    }
    fn release_tracker(&self, mut tracker: Tracker) {
        tracker.clear();
        let mut free = self.free_encoders.lock();
        // Bound retained CPU table storage even if an application records many
        // command buffers. Saturation drops an empty tracker, never waits.
        if free.trackers.len() < 8 { free.trackers.push(tracker); }
    }
}
pub(crate) struct PooledTracker {
    inner: Option<Tracker>,
    device: Arc<Device>,
}
impl Deref for PooledTracker {
    type Target = Tracker;
    fn deref(&self) -> &Tracker { self.inner.as_ref().unwrap() }
}
impl DerefMut for PooledTracker {
    fn deref_mut(&mut self) -> &mut Tracker { self.inner.as_mut().unwrap() }
}
impl Drop for PooledTracker {
    fn drop(&mut self) {
        self.device.command_allocator.release_tracker(self.inner.take().unwrap());
    }
}
