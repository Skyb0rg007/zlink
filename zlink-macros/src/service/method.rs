//! Method information extraction for service macro.

use proc_macro2::TokenStream;
use syn::{
    Attribute, Error, Expr, GenericArgument, Ident, ImplItemFn, Lit, Meta, PathArguments,
    ReturnType, Type,
};

use super::types::ParamInfo;

/// Information extracted from a service method.
pub(super) struct MethodInfo {
    /// The original method name (snake_case).
    pub name: Ident,
    /// The Varlink method name (PascalCase or renamed).
    pub varlink_name: String,
    /// The interface name for this method.
    pub interface: Option<String>,
    /// Parameters for this method (excluding self).
    pub params: Vec<ParamInfo>,
    /// The return type for serialization (the `T` in `Result<T, E>`, or the type itself).
    pub return_type: Option<Type>,
    /// Whether the method returns `Result<T, E>` (true) or just `T` (false).
    pub returns_result: bool,
    /// The error type if the method returns `Result<T, E>` (the `E`).
    pub error_type: Option<Type>,
    /// The method body tokens (for inlining methods with connection params).
    pub body: TokenStream,
}

impl MethodInfo {
    /// Extract method information from an impl method, updating current_interface if a new
    /// interface attribute is found.
    pub(super) fn extract(
        method: &mut ImplItemFn,
        current_interface: &mut Option<String>,
    ) -> Result<Self, Error> {
        let name = method.sig.ident.clone();

        // Extract method attributes.
        let method_attrs = MethodAttrs::extract(&mut method.attrs)?;

        // Update current interface if this method specifies one.
        if let Some(ref iface) = method_attrs.interface {
            *current_interface = Some(iface.clone());
        }

        // Determine the Varlink method name.
        let varlink_name = method_attrs
            .rename
            .unwrap_or_else(|| snake_case_to_pascal_case(&name.to_string()));

        // Extract parameters (skip self).
        let params = method
            .sig
            .inputs
            .iter()
            .skip(1)
            .filter_map(|arg| {
                let mut param_info = ParamInfo::from_fn_arg(arg)?;
                // Try to extract zlink attributes from the parameter.
                if let syn::FnArg::Typed(pat_type) = arg {
                    let param_attrs = extract_param_attrs(&pat_type.attrs);
                    param_info.serialized_name = param_attrs.rename;
                    param_info.is_connection = param_attrs.is_connection;
                }
                Some(param_info)
            })
            .collect();

        // Extract return type and check if it's a Result.
        let (return_type, returns_result, error_type) = match &method.sig.output {
            ReturnType::Default => (None, false, None),
            ReturnType::Type(_, ty) => {
                if let Some((inner_ty, err_ty)) = extract_result_types(ty) {
                    (inner_ty, true, Some(err_ty))
                } else {
                    (Some((**ty).clone()), false, None)
                }
            }
        };

        // Capture the method body.
        let block = &method.block;
        let body = quote::quote! { #block };

        Ok(Self {
            name,
            varlink_name,
            interface: current_interface.clone(),
            params,
            return_type,
            returns_result,
            error_type,
            body,
        })
    }

    /// Get the full Varlink method path (interface.MethodName).
    pub(super) fn full_method_path(&self) -> Option<String> {
        self.interface
            .as_ref()
            .map(|iface| format!("{}.{}", iface, self.varlink_name))
    }

    /// Check if this method has a connection parameter.
    pub(super) fn has_connection_param(&self) -> bool {
        self.params.iter().any(|p| p.is_connection)
    }

    /// Get parameters that are not connection parameters (for serialization).
    pub(super) fn serialized_params(&self) -> impl Iterator<Item = &ParamInfo> {
        self.params.iter().filter(|p| !p.is_connection)
    }
}

/// Attributes extracted from method-level `#[zlink(...)]`.
#[derive(Default)]
struct MethodAttrs {
    /// The interface name for this method.
    interface: Option<String>,
    /// Custom method name.
    rename: Option<String>,
}

impl MethodAttrs {
    /// Extract method attributes from a method's attribute list, removing processed attributes.
    fn extract(attrs: &mut Vec<Attribute>) -> Result<Self, Error> {
        let mut result = Self::default();
        let mut indices_to_remove = Vec::new();

        for (i, attr) in attrs.iter().enumerate() {
            if !attr.path().is_ident("zlink") {
                continue;
            }

            indices_to_remove.push(i);

            let Meta::List(list) = &attr.meta else {
                continue;
            };

            if list.tokens.is_empty() {
                continue;
            }

            let nested = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            )?;

            for meta in nested {
                match &meta {
                    Meta::NameValue(nv) if nv.path.is_ident("interface") => {
                        let Expr::Lit(expr_lit) = &nv.value else {
                            return Err(Error::new_spanned(
                                &nv.value,
                                "interface value must be a string literal",
                            ));
                        };
                        let Lit::Str(lit_str) = &expr_lit.lit else {
                            return Err(Error::new_spanned(
                                &nv.value,
                                "interface value must be a string literal",
                            ));
                        };
                        result.interface = Some(lit_str.value());
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                        let Expr::Lit(expr_lit) = &nv.value else {
                            return Err(Error::new_spanned(
                                &nv.value,
                                "rename value must be a string literal",
                            ));
                        };
                        let Lit::Str(lit_str) = &expr_lit.lit else {
                            return Err(Error::new_spanned(
                                &nv.value,
                                "rename value must be a string literal",
                            ));
                        };
                        result.rename = Some(lit_str.value());
                    }
                    _ => {
                        return Err(Error::new_spanned(&meta, "unknown zlink attribute"));
                    }
                }
            }
        }

        // Remove zlink attributes in reverse order to preserve indices.
        for &index in indices_to_remove.iter().rev() {
            attrs.remove(index);
        }

        Ok(result)
    }
}

/// Attributes extracted from parameter-level `#[zlink(...)]`.
#[derive(Default)]
struct ParamAttrs {
    /// Custom serialized name for the parameter.
    rename: Option<String>,
    /// Whether this parameter should receive the connection.
    is_connection: bool,
}

/// Extract zlink attributes from parameter attributes.
fn extract_param_attrs(attrs: &[Attribute]) -> ParamAttrs {
    let mut result = ParamAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("zlink") {
            continue;
        }

        let Meta::List(list) = &attr.meta else {
            continue;
        };

        let Ok(nested) = list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match &meta {
                Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                    if let Expr::Lit(expr_lit) = &nv.value {
                        if let Lit::Str(lit_str) = &expr_lit.lit {
                            result.rename = Some(lit_str.value());
                        }
                    }
                }
                Meta::Path(path) if path.is_ident("connection") => {
                    result.is_connection = true;
                }
                _ => {}
            }
        }
    }

    result
}

/// Convert snake_case to PascalCase.
fn snake_case_to_pascal_case(input: &str) -> String {
    input
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
        })
        .collect()
}

/// Extract `T` and `E` types from `Result<T, E>`. Returns `None` if the type is not a `Result`.
/// Returns `Some((None, E))` if the Result's Ok type is `()`.
fn extract_result_types(ty: &Type) -> Option<(Option<Type>, Type)> {
    let Type::Path(type_path) = ty else {
        return None;
    };

    let last_segment = type_path.path.segments.last()?;
    if last_segment.ident != "Result" {
        return None;
    }

    let PathArguments::AngleBracketed(args) = &last_segment.arguments else {
        return None;
    };

    // Get the first generic argument (the Ok type).
    let first_arg = args.args.first()?;
    let GenericArgument::Type(ok_type) = first_arg else {
        return None;
    };

    // Get the second generic argument (the Err type).
    let second_arg = args.args.iter().nth(1)?;
    let GenericArgument::Type(err_type) = second_arg else {
        return None;
    };

    // Check if the Ok type is `()`.
    let ok_type = if let Type::Tuple(tuple) = ok_type {
        if tuple.elems.is_empty() {
            None
        } else {
            Some(ok_type.clone())
        }
    } else {
        Some(ok_type.clone())
    };

    Some((ok_type, err_type.clone()))
}
