use fred::types::Expiration;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
struct User {
    id: u64,
    name: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct UserParam {
    id: u64,
}

#[mcache::get("user:{id}-{token}", ttl = 10000)]
fn fetch_user(id: u64, token: String, cache_missed: &mut bool) -> Option<User> {
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

#[test]
fn test_fetch_user_sync() {
    let mut cache_missed = false;
    if let Some(user) = fetch_user(1, "1".to_string(), &mut cache_missed) {
        assert_eq!(user.id, 1);
    } else {
        assert!(false);
    }
}
