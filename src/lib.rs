use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, AttributeArgs, ItemFn, Lit, Meta, NestedMeta};

/// 属性宏：缓存读取，使用环境变量 MCACHE_REDIS_URI
/// 用法：
/// #[get("user:{id}-{name}", ttl = 10000)] // TTL 单位：毫秒
#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 1. 解析宏参数：key 模板和 ttl（毫秒）
    let args = parse_macro_input!(attr as AttributeArgs);
    let mut key_template = String::new();
    let mut ttl = 60_000u64;
    for nested in args {
        match nested {
            NestedMeta::Lit(Lit::Str(lit)) => key_template = lit.value(),
            NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("ttl") => {
                if let Lit::Int(lit_int) = nv.lit {
                    ttl = lit_int.base10_parse().expect("Invalid ttl value");
                }
            }
            _ => {}
        }
    }

    // 2. 解析目标函数
    let function = parse_macro_input!(item as ItemFn);
    let fn_vis = &function.vis;
    let fn_sig = &function.sig;
    let fn_block = &function.block;

    // 收集所有参数标识符，用于占位替换
    let idents: Vec<_> = function
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(pat_type) = arg {
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    return Some(pat_ident.ident.clone());
                }
            }
            None
        })
        .collect();
    if idents.is_empty() {
        panic!("Function must have at least one parameter");
    }

    // 3. 判断是否 async
    let is_async = fn_sig.asyncness.is_some();

    // 4. 根据异步或同步生成不同展开逻辑
    let expanded = if is_async {
        quote! {
            #fn_vis #fn_sig {
                // 读取 Redis URI
                let uri = std::env::var("MCACHE_REDIS_URI").expect("Env MCACHE_REDIS_URI not set");
                let client = redis::Client::open(uri).expect("Redis URI invalid");
                let mut conn = client.get_connection().expect("Failed to connect Redis");

                // 构造缓存键，多参数占位
                let mut cache_key = #key_template.to_string();
                #(
                    cache_key = cache_key.replace(
                        concat!("{", stringify!(#idents), "}"),
                        &#idents.to_string(),
                    );
                )*

                // 尝试命中缓存
                if let Ok(cached) = redis::cmd("GET").arg(&cache_key).query::<String>(&mut conn) {
                    return serde_json::from_str(&cached).unwrap();
                }

                // Cache miss，执行业务逻辑
                let result = (async move #fn_block).await;

                // 写入缓存（毫秒 TTL）
                let serialized = serde_json::to_string(&result).unwrap();
                let _: () = redis::cmd("PSETEX")
                    .arg(&cache_key)
                    .arg(#ttl)
                    .arg(serialized)
                    .query(&mut conn)
                    .unwrap();

                result
            }
        }
    } else {
        quote! {
            #fn_vis #fn_sig {
                let uri = std::env::var("MCACHE_REDIS_URI").expect("Env MCACHE_REDIS_URI not set");
                let client = redis::Client::open(uri).expect("Redis URI invalid");
                let mut conn = client.get_connection().expect("Failed to connect Redis");

                let mut cache_key = #key_template.to_string();
                #(
                    cache_key = cache_key.replace(
                        concat!("{", stringify!(#idents), "}"),
                        &#idents.to_string(),
                    );
                )*

                if let Ok(cached) = redis::cmd("GET").arg(&cache_key).query::<String>(&mut conn) {
                    return serde_json::from_str(&cached).unwrap();
                }

                // 同步 Cache miss
                let result = (|| #fn_block)();

                let serialized = serde_json::to_string(&result).unwrap();
                let _: () = redis::cmd("PSETEX")
                    .arg(&cache_key)
                    .arg(#ttl)
                    .arg(serialized)
                    .query(&mut conn)
                    .unwrap();

                result
            }
        }
    };

    TokenStream::from(expanded)
}
