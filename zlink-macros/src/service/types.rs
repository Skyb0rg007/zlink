//! Types used in service macro processing.

use std::borrow::Cow;

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
    /// Whether this parameter is marked with `#[zlink(fds)]`.
    pub is_fds: bool,
    /// Whether this is the `more` parameter for streaming methods.
    pub is_more: bool,
}

impl ParamInfo {
    /// The parameter name used on the wire (and in the IDL).
    ///
    /// This is the explicit `#[zlink(rename = "...")]` name if provided, the Rust parameter
    /// name otherwise. Parameter names starting with `_` are rejected at extraction time, so
    /// this is always a valid Varlink field name.
    pub(super) fn wire_name(&self) -> Cow<'_, str> {
        match &self.serialized_name {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned(self.name.to_string()),
        }
    }

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
            is_fds: false,
            is_more: false,
        })
    }
}
