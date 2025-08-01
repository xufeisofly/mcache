use fred::prelude::Pool;
use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};

// A global, mutable slot holding an Arc<Pool>.
// `Mutex` so we can overwrite it repeatedly.
// `Arc` so in-flight operations keep the old pool alive.
static REDIS_POOL: Lazy<Mutex<Option<Arc<Pool>>>> = Lazy::new(|| Mutex::new(None));

/// Install or replace the global pool. Safe to call multiple times.
pub fn init(pool: Pool) {
    let mut slot = REDIS_POOL.lock().unwrap();
    *slot = Some(Arc::new(pool));
}

/// Grab the current pool. Panics if you never called `init`.
pub fn pool() -> Arc<Pool> {
    let guard = REDIS_POOL.lock().unwrap();
    guard
        .clone()
        .expect("mcache: pool not initialized; call init() first")
}
