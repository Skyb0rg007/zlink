//! Code generation for the service macro.

use std::collections::{HashMap, HashSet};

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{Error, GenericParam, ItemImpl, Type};

use super::{attrs::ServiceAttrs, method::MethodInfo};

/// Context for generating the `handle` method body.
struct HandleBodyContext<'a> {
    crate_path: &'a TokenStream,
    method_call_name: &'a Ident,
    user_methods_name: &'a Ident,
    reply_params_name: &'a Ident,
    reply_error_name: &'a Ident,
    error_type_map: &'a HashMap<String, usize>,
    interfaces: &'a [String],
    type_name: &'a str,
}

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

/// Collect all unique interfaces from methods.
fn collect_interfaces(methods_info: &[MethodInfo]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut interfaces = Vec::new();
    for method in methods_info {
        if let Some(ref iface) = method.interface {
            if seen.insert(iface.clone()) {
                interfaces.push(iface.clone());
            }
        }
    }
    interfaces
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
    // All generated types use `__` prefix to indicate they are internal implementation details.
    let type_name = extract_type_name(self_ty).unwrap_or_else(|| "Service".to_string());
    let method_call_name = format_ident!("__{}MethodCall", type_name);
    let user_methods_name = format_ident!("__{}UserMethods", type_name);
    let reply_params_name = format_ident!("__{}ReplyParams", type_name);
    let reply_error_name = format_ident!("__{}ReplyError", type_name);

    // Collect interfaces for introspection.
    let interfaces = collect_interfaces(methods_info);

    // Generate the MethodCall enum (outer untagged wrapper + inner user methods).
    let method_call_enum = generate_method_call_enum(
        methods_info,
        &method_call_name,
        &user_methods_name,
        crate_path,
    )?;

    // Generate the Reply enum for parameters.
    let reply_params_enum = generate_reply_params_enum(
        methods_info,
        &method_call_name,
        &reply_params_name,
        crate_path,
    )?;

    // Generate the combo ReplyError enum (always include varlink_service::Error).
    let (reply_error_enum, error_type_map) =
        generate_reply_error_enum(methods_info, &reply_error_name, crate_path);

    // Error type is always the reply error enum (includes varlink_service::Error).
    // The lifetime 'ser is used in the Service trait definition.
    let error_type: syn::Type = syn::parse_quote!(#reply_error_name<'ser>);

    // Generate interface description constants.
    let interface_descriptions = generate_interface_descriptions(
        methods_info,
        service_attrs,
        &interfaces,
        crate_path,
        &type_name,
    );

    // Generate the `handle` method body.
    let handle_body_ctx = HandleBodyContext {
        crate_path,
        method_call_name: &method_call_name,
        user_methods_name: &user_methods_name,
        reply_params_name: &reply_params_name,
        reply_error_name: &reply_error_name,
        error_type_map: &error_type_map,
        interfaces: &interfaces,
        type_name: &type_name,
    };
    let handle_body = generate_handle_body(methods_info, service_attrs, &handle_body_ctx)?;

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
        #interface_descriptions

        #method_call_enum

        #reply_params_enum

        #reply_error_enum

        #service_impl
    })
}

/// Generate the MethodCall enum for deserializing incoming calls.
/// This uses an untagged wrapper to combine varlink service methods with user methods.
fn generate_method_call_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
    user_methods_name: &Ident,
    crate_path: &TokenStream,
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

    let unused_variant = format_ident!("__{}Unused", user_methods_name);
    let unknown_variant = format_ident!("__{}Unknown", user_methods_name);

    // Generate the inner user methods enum.
    let user_methods_enum = if variants.is_empty() {
        quote! {
            #[allow(private_interfaces)]
            #[derive(::core::fmt::Debug, ::serde::Deserialize)]
            #[serde(tag = "method", content = "parameters")]
            pub enum #user_methods_name<'__de> {
                #unused_variant(::core::marker::PhantomData<&'__de ()>),
            }
        }
    } else {
        // Note: #[serde(other)] must be on the last variant.
        quote! {
            #[allow(private_interfaces)]
            #[derive(::core::fmt::Debug, ::serde::Deserialize)]
            #[serde(tag = "method", content = "parameters")]
            pub enum #user_methods_name<'__de> {
                #unused_variant(::core::marker::PhantomData<&'__de ()>),
                #(#variants,)*
                #[serde(other)]
                #unknown_variant,
            }
        }
    };

    // Generate the outer untagged wrapper enum.
    // VarlinkService is tried first (specific matches only), then UserMethods.
    let outer_enum = quote! {
        #[allow(private_interfaces)]
        #[derive(::core::fmt::Debug, ::serde::Deserialize)]
        #[serde(untagged)]
        pub enum #enum_name<'__de> {
            #[serde(borrow)]
            __VarlinkService(#crate_path::varlink_service::Method<'__de>),
            __UserMethods(#user_methods_name<'__de>),
        }
    };

    Ok(quote! {
        #user_methods_enum

        #outer_enum
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
/// Always includes varlink_service::Error for introspection support.
/// Returns the enum definition, From impls, and the error type map.
fn generate_reply_error_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
    crate_path: &TokenStream,
) -> (TokenStream, HashMap<String, usize>) {
    let error_type_map = build_error_type_variant_map(methods_info);

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
                    impl ::core::convert::From<#error_type> for #enum_name<'_> {
                        fn from(e: #error_type) -> Self {
                            #enum_name::#variant_name(e)
                        }
                    }
                });
                break;
            }
        }
    }

    // Always add varlink_service::Error variant for introspection.
    let varlink_error_variant = format_ident!("__{}VarlinkService", enum_name);

    let enum_def = quote! {
        #[allow(private_interfaces)]
        #[derive(::core::fmt::Debug, ::serde::Serialize)]
        #[serde(untagged)]
        pub enum #enum_name<'__ser> {
            #varlink_error_variant(#crate_path::varlink_service::Error<'__ser>),
            #(#variants,)*
        }

        impl<'__ser> ::core::convert::From<#crate_path::varlink_service::Error<'__ser>>
            for #enum_name<'__ser>
        {
            fn from(e: #crate_path::varlink_service::Error<'__ser>) -> Self {
                #enum_name::#varlink_error_variant(e)
            }
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
    crate_path: &TokenStream,
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

    // Always add varlink service reply variant for introspection.
    let varlink_reply_variant = format_ident!("__{}VarlinkService", enum_name);

    if variants.is_empty() {
        return Ok(quote! {
            #[allow(private_interfaces)]
            #[derive(::core::fmt::Debug, ::serde::Serialize)]
            #[serde(untagged)]
            pub enum #enum_name<'__ser> {
                #varlink_reply_variant(#crate_path::varlink_service::Reply<'__ser>),
                #unused_variant(::core::marker::PhantomData<&'__ser ()>),
            }
        });
    }

    Ok(quote! {
        #[allow(private_interfaces)]
        #[derive(::core::fmt::Debug, ::serde::Serialize)]
        #[serde(untagged)]
        pub enum #enum_name<'__ser> {
            #varlink_reply_variant(#crate_path::varlink_service::Reply<'__ser>),
            #(#variants,)*
            #unused_variant(::core::marker::PhantomData<&'__ser ()>),
        }
    })
}

/// Generate interface description constants for each interface.
fn generate_interface_descriptions(
    methods_info: &[MethodInfo],
    service_attrs: &ServiceAttrs,
    interfaces: &[String],
    crate_path: &TokenStream,
    type_name: &str,
) -> TokenStream {
    let mut descriptions: Vec<TokenStream> = Vec::new();

    for interface in interfaces {
        let const_name = format_ident!(
            "__{}_INTERFACE_{}",
            type_name.to_uppercase(),
            interface.replace('.', "_").to_uppercase()
        );

        // Collect methods for this interface.
        let interface_methods: Vec<&MethodInfo> = methods_info
            .iter()
            .filter(|m| m.interface.as_ref() == Some(interface))
            .collect();

        // Generate inner const method definitions.
        // Each method needs its own const to avoid destructor issues.
        let method_consts: Vec<TokenStream> = interface_methods
            .iter()
            .enumerate()
            .map(|(idx, method)| {
                let method_const_name = format_ident!("__METHOD_{}", idx);
                let method_name = &method.varlink_name;

                // Input parameters (excluding connection params).
                let in_params: Vec<TokenStream> = method
                    .serialized_params()
                    .map(|p| {
                        // Strip leading underscores from parameter names for IDL (Rust convention
                        // uses `_name` for unused params, but Varlink IDL doesn't allow that).
                        let default_name = p.name.to_string().trim_start_matches('_').to_string();
                        let param_name = p.serialized_name.as_ref().unwrap_or(&default_name);
                        let ty = &p.ty;
                        quote! {
                            &#crate_path::idl::Parameter::new(
                                #param_name,
                                <#ty as #crate_path::introspect::Type>::TYPE,
                                &[],
                            )
                        }
                    })
                    .collect();

                // Generate a const for this method's in params slice.
                let in_params_const = if in_params.is_empty() {
                    quote! {
                        const __IN_PARAMS: &[&#crate_path::idl::Parameter<'static>] = &[];
                    }
                } else {
                    quote! {
                        const __IN_PARAMS: &[&#crate_path::idl::Parameter<'static>] =
                            &[#(#in_params),*];
                    }
                };

                quote! {
                    const #method_const_name: &#crate_path::idl::Method<'static> = &{
                        #in_params_const
                        #crate_path::idl::Method::new(
                            #method_name,
                            __IN_PARAMS,
                            &[],
                            &[],
                        )
                    };
                }
            })
            .collect();

        // Generate the list of method references.
        let method_refs: Vec<TokenStream> = (0..interface_methods.len())
            .map(|idx| {
                let method_const_name = format_ident!("__METHOD_{}", idx);
                quote! { #method_const_name }
            })
            .collect();

        // Collect custom types for this interface.
        let custom_types: Vec<TokenStream> = service_attrs
            .custom_types
            .iter()
            .map(|ty| {
                quote! {
                    <#ty as #crate_path::introspect::CustomType>::CUSTOM_TYPE
                }
            })
            .collect();

        // Collect error types for this interface (deduplicate using string representation).
        let mut seen_error_types = HashSet::new();
        let error_types: Vec<TokenStream> = methods_info
            .iter()
            .filter(|m| m.interface.as_ref() == Some(interface))
            .filter_map(|m| m.error_type.as_ref())
            .filter(|err_ty| {
                let type_str = quote!(#err_ty).to_string();
                seen_error_types.insert(type_str)
            })
            .map(|err_ty| {
                quote! {
                    <#err_ty as #crate_path::introspect::ReplyError>::VARIANTS
                }
            })
            .collect();

        // Flatten error variants into a single slice.
        let error_variants_expr = if error_types.is_empty() {
            quote! { &[] }
        } else {
            // For simplicity, we collect the first error type's variants.
            // In practice, a service usually has one error type per interface.
            let first_err = &error_types[0];
            quote! { #first_err }
        };

        descriptions.push(quote! {
            #[doc(hidden)]
            const #const_name: &#crate_path::idl::Interface<'static> = &{
                #(#method_consts)*
                #crate_path::idl::Interface::new(
                    #interface,
                    &[#(#method_refs),*],
                    &[#(#custom_types),*],
                    #error_variants_expr,
                    &[],
                )
            };
        });
    }

    quote! { #(#descriptions)* }
}

/// Generate the `handle` method body with match arms.
fn generate_handle_body(
    methods_info: &[MethodInfo],
    service_attrs: &ServiceAttrs,
    ctx: &HandleBodyContext<'_>,
) -> Result<TokenStream, Error> {
    let HandleBodyContext {
        crate_path,
        method_call_name,
        user_methods_name,
        reply_params_name,
        reply_error_name,
        error_type_map,
        interfaces,
        type_name,
    } = ctx;

    let mut user_match_arms: Vec<TokenStream> = Vec::new();
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
            quote! { #user_methods_name::#enum_variant_name }
        } else {
            let param_names: Vec<_> = serialized_params.iter().map(|p| &p.name).collect();
            quote! { #user_methods_name::#enum_variant_name { #(#param_names),* } }
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
                    async move #body
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

        user_match_arms.push(quote! {
            #pattern => {
                #return_expr
            }
        });
    }

    let unused_variant = format_ident!("__{}Unused", user_methods_name);
    let unknown_variant = format_ident!("__{}Unknown", user_methods_name);
    let varlink_error_variant = format_ident!("__{}VarlinkService", reply_error_name);
    let varlink_reply_variant = format_ident!("__{}VarlinkService", reply_params_name);

    // Add the unused variant arm first (to match enum order).
    user_match_arms.insert(
        0,
        quote! {
            #user_methods_name::#unused_variant(_) => {
                unreachable!("unused variant should never be matched")
            }
        },
    );

    // Add a catch-all arm for unknown methods (returns MethodNotFound error).
    user_match_arms.push(quote! {
        #user_methods_name::#unknown_variant => {
            #crate_path::service::MethodReply::Error(
                #reply_error_name::#varlink_error_variant(
                    #crate_path::varlink_service::Error::MethodNotFound {
                        method: ::std::borrow::Cow::Borrowed("unknown"),
                    }
                )
            )
        }
    });

    // Generate the user methods match.
    let user_methods_match = quote! {
        #method_call_name::__UserMethods(__user_method) => {
            match __user_method {
                #(#user_match_arms)*
            }
        }
    };

    // Generate interface description match arms for GetInterfaceDescription.
    let interface_match_arms: Vec<TokenStream> = interfaces
        .iter()
        .map(|interface| {
            let const_name = format_ident!(
                "__{}_INTERFACE_{}",
                type_name.to_uppercase(),
                interface.replace('.', "_").to_uppercase()
            );
            quote! {
                #interface => {
                    let desc = #crate_path::varlink_service::InterfaceDescription::from(#const_name);
                    #crate_path::service::MethodReply::Single(Some(
                        #reply_params_name::#varlink_reply_variant(
                            #crate_path::varlink_service::Reply::InterfaceDescription(desc)
                        )
                    ))
                }
            }
        })
        .collect();

    // Build the interfaces list for GetInfo.
    let interfaces_list: Vec<TokenStream> =
        interfaces.iter().map(|iface| quote! { #iface }).collect();

    // Service metadata.
    let vendor = service_attrs.vendor.as_deref().unwrap_or("");
    let product = service_attrs.product.as_deref().unwrap_or("");
    let version = service_attrs.version.as_deref().unwrap_or("");
    let url = service_attrs.url.as_deref().unwrap_or("");

    // Generate the varlink service methods match.
    let varlink_service_match = quote! {
        #method_call_name::__VarlinkService(__varlink_method) => {
            match __varlink_method {
                #crate_path::varlink_service::Method::GetInfo => {
                    let info = #crate_path::varlink_service::Info::new(
                        #vendor,
                        #product,
                        #version,
                        #url,
                        ::std::vec![
                            #(#interfaces_list,)*
                            #crate_path::varlink_service::INTERFACE_NAME,
                        ],
                    );
                    #crate_path::service::MethodReply::Single(Some(
                        #reply_params_name::#varlink_reply_variant(
                            #crate_path::varlink_service::Reply::Info(info)
                        )
                    ))
                }
                #crate_path::varlink_service::Method::GetInterfaceDescription { interface } => {
                    match *interface {
                        #(#interface_match_arms)*
                        #crate_path::varlink_service::INTERFACE_NAME => {
                            let desc = #crate_path::varlink_service::InterfaceDescription::from(
                                #crate_path::varlink_service::DESCRIPTION
                            );
                            #crate_path::service::MethodReply::Single(Some(
                                #reply_params_name::#varlink_reply_variant(
                                    #crate_path::varlink_service::Reply::InterfaceDescription(desc)
                                )
                            ))
                        }
                        _ => {
                            #crate_path::service::MethodReply::Error(
                                #reply_error_name::#varlink_error_variant(
                                    #crate_path::varlink_service::Error::InterfaceNotFound {
                                        interface: ::std::borrow::Cow::Borrowed(interface),
                                    }
                                )
                            )
                        }
                    }
                }
            }
        }
    };

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
            #varlink_service_match
            #user_methods_match
        }
    })
}
