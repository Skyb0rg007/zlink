//! Service macro attribute parsing.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse::Parser, Error, ItemImpl};

/// Attributes parsed from the `#[service(...)]` macro invocation.
pub(super) struct ServiceAttrs {
    /// The crate path to use (defaults to `::zlink`).
    pub crate_path: TokenStream,
}

impl ServiceAttrs {
    /// Parse attributes from the macro attribute token stream.
    pub(super) fn parse(attr: &TokenStream, item_impl: &ItemImpl) -> Result<Self, Error> {
        let mut crate_path = None;

        if !attr.is_empty() {
            let parser = syn::meta::parser(|meta| {
                if meta.path.is_ident("crate") {
                    let value: syn::LitStr = meta.value()?.parse()?;
                    let path_str = value.value();
                    crate_path = Some(syn::parse_str(&path_str)?);
                } else {
                    return Err(meta.error("unsupported service attribute"));
                }
                Ok(())
            });

            parser.parse2(attr.clone()).map_err(|e| {
                Error::new_spanned(
                    item_impl,
                    format!(
                        "failed to parse service attributes: {e}. Expected: \
                         #[service] or #[service(crate = \"path\")]"
                    ),
                )
            })?;
        }

        Ok(Self {
            crate_path: crate_path.unwrap_or_else(|| quote! { ::zlink }),
        })
    }
}
