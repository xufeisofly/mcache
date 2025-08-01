use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Expr, ExprAsync, LitInt, Token,
};

/// Parse input of the form:
///     get!( cache_key_expr, ttl_ms, async { … } )
struct GetMacroInput {
    key: Expr,
    _comma1: Token![,],
    ttl: LitInt,
    _comma2: Token![,],
    body: ExprAsync,
}

impl Parse for GetMacroInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            key: input.parse()?,
            _comma1: input.parse()?,
            ttl: input.parse()?,
            _comma2: input.parse()?,
            body: input.parse()?,
        })
    }
}

#[proc_macro]
pub fn get(item: TokenStream) -> TokenStream {
    let GetMacroInput { key, ttl, body, .. } = parse_macro_input!(item as GetMacroInput);

    // parse TTL into an i64 literal
    let ttl_ms: i64 = ttl.base10_parse().expect("ttl must be integer");

    let expanded = quote! {
        async move {
            // grab the global pool
            let pool = ::mcache::mcache_core::pool();

            // evaluate the user-supplied expression for the key
            // if it's &str, .to_string() turns it into String; if String, into itself.
            let cache_key: String = (#key).to_string();

            // 1️⃣ Try to GET from cache
            if let Ok(cached) = pool.get::<String, _>(&cache_key).await {
                // cache hit → deserialize and return
                return ::mcache::serde_json::from_str(&cached).unwrap();
            }

            // 2️⃣ Cache miss → run user’s async block
            let result = (#body).await;

            // 3️⃣ Write back into cache
            let serialized = ::mcache::serde_json::to_string(&result).unwrap();
            pool
                .set::<(), _, _>(
                    &cache_key,
                    serialized,
                    Some(::mcache::Expiration::PX(#ttl_ms as i64)),
                    None,
                    false,
                )
                .await
                .unwrap();

            // 4️⃣ Return the computed result
            result
        }
    };

    TokenStream::from(expanded)
}
