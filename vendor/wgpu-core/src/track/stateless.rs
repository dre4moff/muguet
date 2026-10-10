use alloc::{sync::Arc, vec::Vec};
use core::slice::Iter;
use crate::FastHashMap;

/// A tracker that holds strong references to resources.
///
/// This is only used to keep resources alive.
#[derive(Debug)]
pub(crate) struct StatelessTracker<T> {
    resources: Vec<Arc<T>>,
    indices: FastHashMap<usize, usize>,
}

impl<T> StatelessTracker<T> {
    pub fn clear(&mut self) { self.indices.clear(); self.resources.clear(); }

    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            indices: FastHashMap::default(),
        }
    }

    /// Inserts a single resource into the resource tracker.
    ///
    /// Returns a reference to the newly inserted resource.
    /// (This allows avoiding a clone/reference count increase in many cases.)
    pub fn insert_single(&mut self, resource: Arc<T>) -> &Arc<T> {
        let key = Arc::as_ptr(&resource) as usize;
        let next = self.resources.len();
        let index = *self.indices.entry(key).or_insert(next);
        if index == next { self.resources.push(resource); }
        &self.resources[index]
    }
}

impl<'a, T> IntoIterator for &'a StatelessTracker<T> {
    type Item = &'a Arc<T>;
    type IntoIter = Iter<'a, Arc<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.resources.as_slice().iter()
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn interleaved_duplicates_keep_one_owner_until_clear() {
        let values = [Arc::new(1), Arc::new(2), Arc::new(3)];
        let mut tracker = StatelessTracker::new();
        for i in 0..10_000 { assert_eq!(**tracker.insert_single(values[i%3].clone()), i%3+1); }
        assert_eq!((&tracker).into_iter().count(), 3);
        assert!(values.iter().all(|v| Arc::strong_count(v)==2));
        tracker.clear(); assert!(values.iter().all(|v| Arc::strong_count(v)==1));
        tracker.insert_single(values[2].clone());
        assert_eq!((&tracker).into_iter().count(), 1);
        drop(tracker); assert!(values.iter().all(|v| Arc::strong_count(v)==1));
    }
}
