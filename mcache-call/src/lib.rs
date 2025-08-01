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

    let ttl_ms: i64 = ttl.base10_parse().expect("ttl must be integer");

    let expanded = quote! {
        async move {
            let pool = ::mcache::mcache_core::pool();

            let cache_key: String = (#key).to_string();

            if let Ok(cached) = pool.get::<String, _>(&cache_key).await {
                return ::mcache::serde_json::from_str(&cached).unwrap();
            }

            let result = (#body).await;

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

            result
        }
    };

    TokenStream::from(expanded)
}
