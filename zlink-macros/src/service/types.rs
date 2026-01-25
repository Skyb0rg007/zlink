//! Types used in service macro processing.

use syn::{FnArg, Ident, Pat, Type};

/// Information about a method parameter.
#[derive(Clone)]
pub(super) struct ParamInfo {
    /// The parameter name.
    pub name: Ident,
    /// The parameter type.
    pub ty: Type,
    /// The serialized name (from `#[zlink(rename = "...")]`).
    pub serialized_name: Option<String>,
    /// Whether this parameter is marked with `#[zlink(connection)]`.
    pub is_connection: bool,
}

impl ParamInfo {
    /// Extract parameter information from a function argument.
    pub(super) fn from_fn_arg(arg: &FnArg) -> Option<Self> {
        let FnArg::Typed(pat_type) = arg else {
            return None;
        };
        let Pat::Ident(pat_ident) = &*pat_type.pat else {
            return None;
        };

        Some(Self {
            name: pat_ident.ident.clone(),
            ty: (*pat_type.ty).clone(),
            serialized_name: None,
            is_connection: false,
        })
    }
}
