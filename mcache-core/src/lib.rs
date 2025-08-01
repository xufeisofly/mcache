use fred::prelude::Pool;
use once_cell::sync::OnceCell;

static REDIS_POOL: OnceCell<Pool> = OnceCell::new();

pub fn init(pool: Pool) {
    REDIS_POOL
        .set(pool)
        .expect("mcache: Pool alread initialized");
}

pub fn pool() -> &'static Pool {
    REDIS_POOL
        .get()
        .expect("mcache: Pool not initialized (call mcache_core::init first)")
}
