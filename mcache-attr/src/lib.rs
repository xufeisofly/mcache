use proc_macro::TokenStream;
use quote::quote;
use regex::Regex;
use syn::{parse_macro_input, AttributeArgs, ItemFn, Lit, Meta, NestedMeta};

/// #[get("user:{id}-{name}", ttl = 10000)] // TTL unit: ms
#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    // parse the key template and ttl
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

    // parse the function
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

    // if it is an async function
    if !fn_sig.asyncness.is_some() {
        return syn::Error::new_spanned(
            &fn_sig.fn_token,
            "`#[get]` cannot be used on sync functions.",
        )
        .to_compile_error()
        .into();
    }

    let expanded = {
        quote! {
            #fn_vis #fn_sig {
                let pool = ::mcache::mcache_core::pool();

                let mut cache_key = #key_template.to_string();
                #(#replaces)*

                if let Ok(cached) = pool.get::<String, _>(&cache_key).await {
                    return ::mcache::serde_json::from_str(&cached).unwrap();
                }

                let result = (async move #fn_block).await;

                let serialized = ::mcache::serde_json::to_string(&result).unwrap();
                pool
                    .set::<(), _, _>(
                        &cache_key,
                        serialized,
                        Some(::mcache::Expiration::PX(#ttl as i64)),
                        None,
                        false,
                    )
                    .await.unwrap();
                result
            }
        }
    };

    TokenStream::from(expanded)
}

// #[proc_macro_attribute]
// pub fn get_sync(attr: TokenStream, item: TokenStream) -> TokenStream {
//     // parse the key template and ttl
//     let args = parse_macro_input!(attr as AttributeArgs);
//     let mut key_template = String::new();
//     let mut ttl = 60_000u64;
//     for nested in args {
//         match nested {
//             NestedMeta::Lit(Lit::Str(lit)) => key_template = lit.value(),
//             NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("ttl") => {
//                 if let Lit::Int(lit_int) = nv.lit {
//                     ttl = lit_int.base10_parse().expect("Invalid ttl value");
//                 }
//             }
//             _ => {}
//         }
//     }

//     // parse the function
//     let function = parse_macro_input!(item as ItemFn);
//     let fn_vis = &function.vis;
//     let fn_sig = &function.sig;
//     let fn_block = &function.block;

//     // Grab the raw literal so we can regex it
//     let raw_template = key_template.clone();

//     // Build a regex to find {…} placeholders
//     let re = Regex::new(r"\{([a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)*)\}").unwrap();
//     // We'll collect one Quote‐fragment per match
//     let mut replaces = Vec::new();
//     for caps in re.captures_iter(&raw_template) {
//         let full = &caps[0]; // e.g. "{param.id}"
//         let path = &caps[1]; // e.g.  "param.id"

//         // turn "param.id.whatever" into a syn::Expr so we can generate code
//         let mut segments = path.split('.');
//         let first = syn::Ident::new(segments.next().unwrap(), proc_macro2::Span::call_site());
//         let mut expr = quote! { #first };
//         for segment in segments {
//             let seg = syn::Ident::new(segment, proc_macro2::Span::call_site());
//             expr = quote! { #expr.#seg };
//         }

//         // now generate a `cache_key = cache_key.replace("{param.id}", &param.id.to_string());`
//         replaces.push(quote! {
//             cache_key = cache_key.replace(
//                 #full,
//                 &(#expr).to_string(),
//             );
//         });
//     }

//     if fn_sig.asyncness.is_some() {
//         return syn::Error::new_spanned(
//             &fn_sig.fn_token,
//             "`#[get_sync]` cannot be used on async functions. Use `#[get]` instead.",
//         )
//         .to_compile_error()
//         .into();
//     }

//     let expanded = quote! {
//       #fn_vis #fn_sig {
//         let pool = ::mcache::mcache_core::pool();

//         // Build the key once — a String we own
//         let cache_key = #key_template.to_string();
//         #(#replaces)*

//         // 1️⃣ Try GET
//         let maybe_cached: Option<String> = if let Ok(handle) = ::tokio::runtime::Handle::try_current() {
//           // inside a Tokio runtime: block in place and await the future
//           ::tokio::task::block_in_place(|| {
//             handle.block_on(pool.get::<String, _>(&cache_key))
//           })
//           .ok()
//           .flatten()
//         } else {
//           // outside any runtime: spin up one just for this call
//           ::tokio::runtime::Runtime::new()
//             .unwrap()
//             .block_on(pool.get::<String, _>(&cache_key))
//             .ok()
//             .flatten()
//         };

//         if let Some(cached_json) = maybe_cached {
//           // parse and return without ever cloning the key again
//           return ::mcache::serde_json::from_str(&cached_json).unwrap();
//         }

//         // 2️⃣ Cache miss → run the user’s sync body
//         let result = (|| #fn_block)();

//         // 3️⃣ Serialize & SET
//         let serialized = ::mcache::serde_json::to_string(&result).unwrap();
//         if let Ok(handle) = ::tokio::runtime::Handle::try_current() {
//           ::tokio::task::block_in_place(|| {
//             handle.block_on(
//               pool.set(
//                 &cache_key,
//                 serialized,
//                 Some(::mcache::Expiration::PX(#ttl as i64)),
//                 None,
//                 false,
//               )
//             )
//           })
//           .unwrap();
//         } else {
//           ::tokio::runtime::Runtime::new()
//             .unwrap()
//             .block_on(
//               pool.set(
//                 &cache_key,
//                 serialized,
//                 Some(::mcache::Expiration::PX(#ttl as i64)),
//                 None,
//                 false,
//               )
//             )
//             .unwrap();
//         }

//         result
//       }
//     };

//     let expanded = quote! {
//         #fn_vis #fn_sig {
//             let pool = ::mcache::mcache_core::pool();
//             let mut cache_key = #key_template.to_string();
//             #(#replaces)*

//             // 封装一个 async 块，统一执行
//             let fut = async move {
//                 if let Ok(cached) = pool.get::<String, _>(&cache_key).await {
//                     return ::mcache::serde_json::from_str(&cached).unwrap();
//                 }
//                 None
//             };

//             // 判断是否在 tokio runtime 里
//             let cached_value = if let Ok(handle) = ::tokio::runtime::Handle::try_current() {
//                 ::tokio::task::block_in_place(|| {
//                     handle.block_on(fut)
//                 })

//             } else {
//                 ::tokio::runtime::Runtime::new().unwrap().block_on(fut)
//             };

//             if let Some(value) = cached_value {
//                 return value;
//             }

//             // 没命中缓存，执行原函数
//             let result = (|| #fn_block)();

//             // 再把写缓存的逻辑放到 async 里
//             let set_fut = async move {
//                 let serialized = ::mcache::serde_json::to_string(&result).unwrap();
//                 pool.set::<(), _, _>(
//                     &cache_key,
//                     serialized,
//                     Some(::mcache::Expiration::PX(#ttl as i64)),
//                     None,
//                     false,
//                 ).await.unwrap();
//             };

//             if let Ok(handle) = ::tokio::runtime::Handle::try_current() {
//                 ::tokio::task::block_in_place(|| {
//                     handle.block_on(fut)
//                 });
//             } else {
//                 ::tokio::runtime::Runtime::new().unwrap().block_on(set_fut);
//             }

//             result
//         }
//     };

//     TokenStream::from(expanded)
// }
