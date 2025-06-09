use std::{any::Any, borrow::Borrow, hash::Hash, sync::Arc, time::Duration};

use moka::sync::Cache;

/// Note that values are cloned when retrieved
pub struct DynamicCache<K> {
    cache: Cache<K, Arc<dyn Any + Sync + Send>>,
}

impl<K: Eq + Hash + Sync + Send + 'static> DynamicCache<K> {
    pub fn new(capacity: u64, time_to_live: Duration, time_to_idle: Duration) -> Self {
        let cache = Cache::builder()
            .max_capacity(capacity)
            .time_to_live(time_to_live)
            .time_to_idle(time_to_idle)
            .build();

        Self { cache }
    }

    pub fn insert<T>(&self, key: K, request: T)
    where
        T: Sync + Send + Clone + 'static,
    {
        self.cache.insert(key, Arc::new(request));
    }

    pub fn get<Q, T>(&self, key: &Q) -> Option<T>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
        T: Sync + Send + Clone + 'static,
    {
        match self.cache.get(key) {
            None => None,
            Some(cached) => Some((*cached.downcast::<T>().unwrap()).clone()),
        }
    }
}
