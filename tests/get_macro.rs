use fred::prelude::{self as Redis, *};
use mcache::{mcache_attr, mcache_core};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Deserialize, Serialize, Debug)]
struct User {
    id: u64,
    name: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct UserParam {
    id: u64,
    token: String,
}

async fn setup() {
    let redis_config = Redis::Config::from_url("redis://127.0.0.1/").expect("parse redis url");
    let redis_pool = Redis::Builder::from_config(redis_config)
        .with_connection_config(|config| {
            config.connection_timeout = Duration::from_secs(10);
        })
        .set_policy(Redis::ReconnectPolicy::new_exponential(0, 100, 30_000, 100))
        .build_pool(16)
        .expect("build pool");
    redis_pool.init().await.expect("init redis pool");
    mcache_core::init(redis_pool);
}

#[mcache_attr::get("user:{id}-{token}", ttl = 100)]
async fn fetch_user(id: u64, token: String, cache_missed: &mut bool) -> Option<User> {
    println!("Async cache miss...");
    *cache_missed = true;
    if id == 0 {
        return None;
    }
    Some(User {
        id,
        name: format!("async_user{}", token),
    })
}

#[mcache_attr::get("user2:{p.id}-{p.token}", ttl = 100)]
async fn fetch_user_by_struct(p: &UserParam, cache_missed: &mut bool) -> Option<User> {
    println!("Async cache miss by param...");
    *cache_missed = true;
    if p.id == 0 {
        return None;
    }
    Some(User {
        id: p.id,
        name: format!("async_user{}", p.token),
    })
}

#[tokio::test]
async fn test_fetch_user() {
    setup().await;

    let mut missed = false;
    let mut id = 1;
    let mut token = "1";

    assert!(fetch_user(id, token.into(), &mut missed).await.is_some());
    assert!(missed);

    missed = false;
    assert!(fetch_user(id, token.into(), &mut missed).await.is_some());
    assert!(!missed);

    id = 0;
    token = "0";

    assert!(fetch_user(id, token.into(), &mut missed).await.is_none());
    assert!(missed);

    missed = false;
    assert!(fetch_user(id, token.into(), &mut missed).await.is_none());
    assert!(!missed);

    let mut p = UserParam {
        id: 1,
        token: "1".into(),
    };

    assert!(fetch_user_by_struct(&p, &mut missed).await.is_some());
    assert!(missed);

    missed = false;
    assert!(fetch_user_by_struct(&p, &mut missed).await.is_some());
    assert!(!missed);

    p = UserParam {
        id: 0,
        token: "0".into(),
    };

    assert!(fetch_user_by_struct(&p, &mut missed).await.is_none());
    assert!(missed);

    missed = false;
    assert!(fetch_user_by_struct(&p, &mut missed).await.is_none());
    assert!(!missed);
}
