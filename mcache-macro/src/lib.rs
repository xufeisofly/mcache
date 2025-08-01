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
    let is_async = fn_sig.asyncness.is_some();

    let expanded = if is_async {
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
                        Some(Expiration::PX(#ttl as i64)),
                        None,
                        false,
                    )
                    .await.unwrap();
                result
            }
        }
    } else {
        quote! {
            #fn_vis #fn_sig {
                let pool = ::mcache::mcache_core::pool();

                let mut cache_key = #key_template.to_string();
                #(#replaces)*

                if let Ok(cached) = ::mcache::futures::executor::block_on(pool.get::<String, _>(&cache_key)) {
                    return ::mcache::serde_json::from_str(&cached).unwrap();
                }

                let result = (|| #fn_block)();

                let serialized = ::mcache::serde_json::to_string(&result).unwrap();
                ::mcache::futures::executor::block_on(
                    pool
                        .set::<(), _, _>(
                            &cache_key,
                            serialized,
                            Some(Expiration::PX(#ttl as i64)),
                            None,
                            false,
                        )
                ).unwrap();

                result
            }
        }
    };

    TokenStream::from(expanded)
}
