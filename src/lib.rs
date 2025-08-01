use proc_macro::TokenStream;
use quote::quote;
use regex::Regex;
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

    // Grab the raw literal so we can regex it
    let raw_template = key_template.clone();

    // Build a regex to find {…} placeholders
    let re = Regex::new(r"\{([a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)*)\}").unwrap();
    // We'll collect one Quote‐fragment per match
    let mut replaces = Vec::new();
    for caps in re.captures_iter(&raw_template) {
        let full = &caps[0]; // e.g. "{param.id}"
        let path = &caps[1]; // e.g.  "param.id"

        // turn "param.id.whatever" into a syn::Expr so we can generate code
        let mut segments = path.split('.');
        let first = syn::Ident::new(segments.next().unwrap(), proc_macro2::Span::call_site());
        let mut expr = quote! { #first };
        for segment in segments {
            let seg = syn::Ident::new(segment, proc_macro2::Span::call_site());
            expr = quote! { #expr.#seg };
        }

        // now generate a `cache_key = cache_key.replace("{param.id}", &param.id.to_string());`
        replaces.push(quote! {
            cache_key = cache_key.replace(
                #full,
                &(#expr).to_string(),
            );
        });
    }

    // 3. 判断是否 async
    let is_async = fn_sig.asyncness.is_some();

    // 4. 根据异步或同步生成不同展开逻辑
    let expanded = if is_async {
        quote! {
            #fn_vis #fn_sig {
                let mut conn = mcache_core::CLIENT
                    .get_connection()
                    .expect("Failed to connect Redis");

                // 构造缓存键，多参数占位
                let mut cache_key = #key_template.to_string();
                #(#replaces)*

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
                let mut conn = mcache_core::CLIENT
                    .get_connection()
                    .expect("Failed to connect Redis");

                let mut cache_key = #key_template.to_string();
                #(#replaces)*

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
