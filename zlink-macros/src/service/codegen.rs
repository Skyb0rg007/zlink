//! Code generation for the service macro.

use std::collections::HashMap;

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{Error, GenericParam, ItemImpl, Type};

use super::{attrs::ServiceAttrs, method::MethodInfo};

/// Extract a simple type name from a Type for generating auxiliary type names.
fn extract_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|seg| seg.ident.to_string()),
        _ => None,
    }
}

/// Generate the Service trait implementation.
pub(super) fn generate_service_impl(
    item_impl: &ItemImpl,
    methods_info: &[MethodInfo],
    service_attrs: &ServiceAttrs,
) -> Result<TokenStream, Error> {
    let crate_path = &service_attrs.crate_path;
    let self_ty = &item_impl.self_ty;

    // Extract the type name for generating auxiliary type names.
    let type_name = extract_type_name(self_ty).unwrap_or_else(|| "Service".to_string());
    let method_call_name = format_ident!("{}MethodCall", type_name);
    let reply_params_name = format_ident!("{}ReplyParams", type_name);
    let reply_error_name = format_ident!("{}ReplyError", type_name);

    // Generate the MethodCall enum.
    let method_call_enum = generate_method_call_enum(methods_info, &method_call_name)?;

    // Generate the Reply enum for parameters.
    let reply_params_enum =
        generate_reply_params_enum(methods_info, &method_call_name, &reply_params_name)?;

    // Generate the combo ReplyError enum.
    let (reply_error_enum, error_type_map) =
        generate_reply_error_enum(methods_info, &reply_error_name);

    // Determine the error type to use in the Service impl.
    let has_result_methods = methods_info.iter().any(|m| m.returns_result);
    let error_type: syn::Type = if has_result_methods {
        syn::parse_quote!(#reply_error_name)
    } else {
        syn::parse_quote!(())
    };

    // Generate the handle method body.
    let handle_body = generate_handle_body(
        methods_info,
        crate_path,
        &method_call_name,
        &reply_params_name,
        &reply_error_name,
        &error_type_map,
    )?;

    // Extract socket type parameter: use user-provided first type param, or default to __ZlinkSock.
    let (socket_ty, generics, user_where_clause) = item_impl
        .generics
        .params
        .iter()
        .find_map(|p| match p {
            GenericParam::Type(ty) => Some((
                ty.ident.clone(),
                item_impl.generics.clone(),
                item_impl.generics.where_clause.clone(),
            )),
            _ => None,
        })
        .unwrap_or_else(|| {
            let default_ident: Ident = format_ident!("__ZlinkSock");
            let mut generics = syn::Generics::default();
            generics
                .params
                .push(GenericParam::Type(syn::TypeParam::from(
                    default_ident.clone(),
                )));
            (default_ident, generics, None)
        });

    // Build the where clause: always add Socket bound, then user's additional predicates.
    let user_predicates = user_where_clause.map(|w| w.predicates).unwrap_or_default();
    let where_clause = quote! {
        where
            #socket_ty: #crate_path::connection::Socket,
            #user_predicates
    };

    // Generate the impl block.
    let service_impl = quote! {
        impl #generics #crate_path::Service<#socket_ty> for #self_ty
        #where_clause
        {
            type MethodCall<'de> = #method_call_name<'de>;
            type ReplyParams<'ser> = #reply_params_name<'ser> where Self: 'ser;
            type ReplyStream = ::futures_util::stream::Empty<#crate_path::Reply<()>>;
            type ReplyStreamParams = ();
            type ReplyError<'ser> = #error_type where Self: 'ser;

            async fn handle<'__zlink_ser>(
                &'__zlink_ser mut self,
                __zlink_call: &'__zlink_ser #crate_path::Call<Self::MethodCall<'_>>,
                __zlink_conn: &mut #crate_path::Connection<#socket_ty>,
            ) -> #crate_path::service::MethodReply<
                Self::ReplyParams<'__zlink_ser>,
                Self::ReplyStream,
                Self::ReplyError<'__zlink_ser>,
            > {
                #handle_body
            }
        }
    };

    Ok(quote! {
        #method_call_enum

        #reply_params_enum

        #reply_error_enum

        #service_impl
    })
}

/// Generate the MethodCall enum for deserializing incoming calls.
fn generate_method_call_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
) -> Result<TokenStream, Error> {
    let variants: Vec<TokenStream> = methods_info
        .iter()
        .filter_map(|method| {
            let full_path = method.full_method_path()?;
            let variant_name = format_ident!("{}", method.varlink_name);

            // Only include serialized params (exclude connection params).
            let serialized_params: Vec<_> = method.serialized_params().collect();

            let fields = if serialized_params.is_empty() {
                quote! {}
            } else {
                let field_defs: Vec<TokenStream> = serialized_params
                    .iter()
                    .map(|param| {
                        let name = &param.name;
                        let ty = &param.ty;

                        let serde_attr = if let Some(ref renamed) = param.serialized_name {
                            quote! { #[serde(rename = #renamed)] }
                        } else {
                            quote! {}
                        };

                        quote! {
                            #serde_attr
                            #name: #ty
                        }
                    })
                    .collect();

                quote! {
                    { #(#field_defs),* }
                }
            };

            Some(quote! {
                #[serde(rename = #full_path)]
                #variant_name #fields
            })
        })
        .collect();

    let unused_variant = format_ident!("__{}Unused", enum_name);

    if variants.is_empty() {
        return Ok(quote! {
            #[derive(::core::fmt::Debug, ::serde::Deserialize)]
            #[serde(tag = "method", content = "parameters")]
            enum #enum_name<'__de> {
                #[doc(hidden)]
                #unused_variant(::core::marker::PhantomData<&'__de ()>),
            }
        });
    }

    let unknown_variant = format_ident!("__{}Unknown", enum_name);

    // Note: #[serde(other)] must be on the last variant.
    Ok(quote! {
        #[derive(::core::fmt::Debug, ::serde::Deserialize)]
        #[serde(tag = "method", content = "parameters")]
        enum #enum_name<'__de> {
            #[doc(hidden)]
            #unused_variant(::core::marker::PhantomData<&'__de ()>),
            #(#variants,)*
            #[doc(hidden)]
            #[serde(other)]
            #unknown_variant,
        }
    })
}

/// Build a mapping from return type string representation to variant index.
/// This ensures consistent variant naming between reply enum generation and handle body.
fn build_return_type_variant_map(methods_info: &[MethodInfo]) -> HashMap<String, usize> {
    let mut type_to_variant: HashMap<String, usize> = HashMap::new();
    let mut variant_idx = 0;

    for method in methods_info {
        let Some(ref return_type) = method.return_type else {
            continue;
        };

        // Use a simple string representation to check for duplicates.
        let type_str = quote!(#return_type).to_string();
        if let std::collections::hash_map::Entry::Vacant(e) = type_to_variant.entry(type_str) {
            e.insert(variant_idx);
            variant_idx += 1;
        }
    }

    type_to_variant
}

/// Build a mapping from error type string representation to variant index.
/// This ensures consistent variant naming for the combo error enum.
fn build_error_type_variant_map(methods_info: &[MethodInfo]) -> HashMap<String, usize> {
    let mut type_to_variant: HashMap<String, usize> = HashMap::new();
    let mut variant_idx = 0;

    for method in methods_info {
        let Some(ref error_type) = method.error_type else {
            continue;
        };

        // Use a simple string representation to check for duplicates.
        let type_str = quote!(#error_type).to_string();
        if let std::collections::hash_map::Entry::Vacant(e) = type_to_variant.entry(type_str) {
            e.insert(variant_idx);
            variant_idx += 1;
        }
    }

    type_to_variant
}

/// Generate the combo ReplyError enum and From impls for each error type.
/// Returns the enum definition, From impls, and the error type map.
fn generate_reply_error_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
) -> (TokenStream, HashMap<String, usize>) {
    let error_type_map = build_error_type_variant_map(methods_info);

    if error_type_map.is_empty() {
        // No methods return Result, no enum needed.
        return (quote! {}, error_type_map);
    }

    // Build variants in order of their indices.
    let mut type_variant_pairs: Vec<_> = error_type_map.iter().collect();
    type_variant_pairs.sort_by_key(|(_, idx)| *idx);

    let mut variants: Vec<TokenStream> = Vec::new();
    let mut from_impls: Vec<TokenStream> = Vec::new();

    for (type_str, idx) in type_variant_pairs {
        // Find the actual error type from methods.
        for method in methods_info {
            let Some(ref error_type) = method.error_type else {
                continue;
            };
            if &quote!(#error_type).to_string() == type_str {
                let variant_name = format_ident!("__{}Variant{}", enum_name, idx);
                variants.push(quote! {
                    #variant_name(#error_type)
                });

                from_impls.push(quote! {
                    impl ::core::convert::From<#error_type> for #enum_name {
                        fn from(e: #error_type) -> Self {
                            #enum_name::#variant_name(e)
                        }
                    }
                });
                break;
            }
        }
    }

    let unused_variant = format_ident!("__{}Unused", enum_name);

    let enum_def = quote! {
        #[derive(::core::fmt::Debug, ::serde::Serialize)]
        #[serde(untagged)]
        enum #enum_name {
            #(#variants,)*
            #[doc(hidden)]
            #unused_variant,
        }

        #(#from_impls)*
    };

    (enum_def, error_type_map)
}

/// Generate the ReplyParams enum for serializing outgoing replies.
fn generate_reply_params_enum(
    methods_info: &[MethodInfo],
    method_call_name: &Ident,
    enum_name: &Ident,
) -> Result<TokenStream, Error> {
    // Collect unique return types from methods.
    let mut variants: Vec<TokenStream> = Vec::new();
    let type_to_variant = build_return_type_variant_map(methods_info);

    // Build variants in order of their indices.
    let mut type_variant_pairs: Vec<_> = type_to_variant.iter().collect();
    type_variant_pairs.sort_by_key(|(_, idx)| *idx);

    for (type_str, idx) in type_variant_pairs {
        // Find the actual return type from methods.
        for method in methods_info {
            let Some(ref return_type) = method.return_type else {
                continue;
            };
            if &quote!(#return_type).to_string() == type_str {
                let variant_name = format_ident!("__{}Variant{}", method_call_name, idx);
                variants.push(quote! {
                    #variant_name(#return_type)
                });
                break;
            }
        }
    }

    let unused_variant = format_ident!("__{}Unused", enum_name);

    if variants.is_empty() {
        return Ok(quote! {
            #[derive(::core::fmt::Debug, ::serde::Serialize)]
            #[serde(untagged)]
            enum #enum_name<'__ser> {
                #[doc(hidden)]
                #unused_variant(::core::marker::PhantomData<&'__ser ()>),
            }
        });
    }

    Ok(quote! {
        #[derive(::core::fmt::Debug, ::serde::Serialize)]
        #[serde(untagged)]
        enum #enum_name<'__ser> {
            #(#variants,)*
            #[doc(hidden)]
            #unused_variant(::core::marker::PhantomData<&'__ser ()>),
        }
    })
}

/// Generate the handle method body with match arms.
fn generate_handle_body(
    methods_info: &[MethodInfo],
    crate_path: &TokenStream,
    method_call_name: &Ident,
    reply_params_name: &Ident,
    reply_error_name: &Ident,
    error_type_map: &HashMap<String, usize>,
) -> Result<TokenStream, Error> {
    let mut match_arms: Vec<TokenStream> = Vec::new();
    let type_to_variant = build_return_type_variant_map(methods_info);

    for method in methods_info {
        let Some(_full_path) = method.full_method_path() else {
            continue;
        };

        let enum_variant_name = format_ident!("{}", method.varlink_name);
        let method_name = &method.name;

        // Only include serialized params in the pattern (exclude connection params).
        let serialized_params: Vec<_> = method.serialized_params().collect();

        // Build the pattern for the match arm.
        let pattern = if serialized_params.is_empty() {
            quote! { #method_call_name::#enum_variant_name }
        } else {
            let param_names: Vec<_> = serialized_params.iter().map(|p| &p.name).collect();
            quote! { #method_call_name::#enum_variant_name { #(#param_names),* } }
        };

        // Build the method call expression.
        // For methods with connection params, inline the body (the method isn't in the output
        // impl). For other methods, call the method normally.
        let method_call = if method.has_connection_param() {
            // Inline the method body with variable bindings.
            let body = &method.body;

            // Set up bindings for connection params.
            let conn_bindings: Vec<TokenStream> = method
                .params
                .iter()
                .filter(|p| p.is_connection)
                .map(|p| {
                    let name = &p.name;
                    quote! { let #name = __zlink_conn; }
                })
                .collect();

            // Set up bindings for regular params (clone from pattern match).
            let param_bindings: Vec<TokenStream> = method
                .params
                .iter()
                .filter(|p| !p.is_connection)
                .map(|p| {
                    let name = &p.name;
                    quote! { let #name = ::core::clone::Clone::clone(#name); }
                })
                .collect();

            quote! {
                {
                    #(#conn_bindings)*
                    #(#param_bindings)*
                    async #body
                }.await
            }
        } else {
            // Build the method call arguments, cloning values from the pattern match.
            let call_args: Vec<TokenStream> = method
                .params
                .iter()
                .map(|p| {
                    let name = &p.name;
                    quote! { ::core::clone::Clone::clone(#name) }
                })
                .collect();

            quote! { self.#method_name(#(#call_args),*).await }
        };

        // Build the return expression based on method type.
        let return_expr = if method.returns_result {
            // Method returns Result<T, E>. Get the error variant for this method's error type.
            let error_variant = method.error_type.as_ref().map(|err_ty| {
                let type_str = quote!(#err_ty).to_string();
                let variant_idx = error_type_map.get(&type_str).copied().unwrap_or(0);
                format_ident!("__{}Variant{}", reply_error_name, variant_idx)
            });

            if let Some(ref return_type) = method.return_type {
                // Result<T, E> where T is not ().
                let type_str = quote!(#return_type).to_string();
                let variant_idx = type_to_variant.get(&type_str).copied().unwrap_or(0);
                let reply_variant_name =
                    format_ident!("__{}Variant{}", method_call_name, variant_idx);
                let error_convert = if let Some(err_variant) = error_variant {
                    quote! { #reply_error_name::#err_variant(__err) }
                } else {
                    quote! { ::core::convert::From::from(__err) }
                };
                quote! {
                    match #method_call {
                        ::core::result::Result::Ok(__ok) => {
                            #crate_path::service::MethodReply::Single(Some(
                                #reply_params_name::#reply_variant_name(__ok)
                            ))
                        }
                        ::core::result::Result::Err(__err) => {
                            #crate_path::service::MethodReply::Error(#error_convert)
                        }
                    }
                }
            } else {
                // Result<(), E>.
                let error_convert = if let Some(err_variant) = error_variant {
                    quote! { #reply_error_name::#err_variant(__err) }
                } else {
                    quote! { ::core::convert::From::from(__err) }
                };
                quote! {
                    match #method_call {
                        ::core::result::Result::Ok(()) => {
                            #crate_path::service::MethodReply::Single(None)
                        }
                        ::core::result::Result::Err(__err) => {
                            #crate_path::service::MethodReply::Error(#error_convert)
                        }
                    }
                }
            }
        } else if let Some(ref return_type) = method.return_type {
            // Method returns T directly (not a Result).
            let type_str = quote!(#return_type).to_string();
            let variant_idx = type_to_variant.get(&type_str).copied().unwrap_or(0);
            let reply_variant_name = format_ident!("__{}Variant{}", method_call_name, variant_idx);
            quote! {
                let __result = #method_call;
                #crate_path::service::MethodReply::Single(Some(
                    #reply_params_name::#reply_variant_name(__result)
                ))
            }
        } else {
            // Method has no return type.
            quote! {
                let _ = #method_call;
                #crate_path::service::MethodReply::Single(None)
            }
        };

        match_arms.push(quote! {
            #pattern => {
                #return_expr
            }
        });
    }

    let unused_variant = format_ident!("__{}Unused", method_call_name);
    let unknown_variant = format_ident!("__{}Unknown", method_call_name);

    // Add the unused variant arm first (to match enum order).
    match_arms.insert(
        0,
        quote! {
            #method_call_name::#unused_variant(_) => {
                unreachable!("unused variant should never be matched")
            }
        },
    );

    // Add a catch-all arm for unknown methods (last to match enum order).
    match_arms.push(quote! {
        #method_call_name::#unknown_variant => {
            // Unknown method - this should ideally return an error.
            // For now, return None.
            #crate_path::service::MethodReply::Single(None)
        }
    });

    // Check if any method uses the connection parameter.
    let uses_connection = methods_info.iter().any(|m| m.has_connection_param());

    let conn_suppression = if uses_connection {
        // Connection is used, no suppression needed.
        quote! {}
    } else {
        // Suppress unused warning when no methods use the connection.
        quote! { let _ = __zlink_conn; }
    };

    Ok(quote! {
        #conn_suppression
        match __zlink_call.method() {
            #(#match_arms)*
        }
    })
}
