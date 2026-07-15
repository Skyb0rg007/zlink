//! Compile-time assertions for custom type declarations.
//!
//! These const functions are used by the `#[service]` macro to verify at compile time that
//! any custom types referenced in method signatures are declared in `types = [...]`.

use crate::idl;

/// Assert at compile time that every parameter in `params` whose type is a custom type
/// reference has its name listed in `declared`.
///
/// This is the batch version of [`assert_type_declared`] used for checking method output
/// parameters, where the full `&[&Parameter]` slice is available as a const.
///
/// # Panics
///
/// Panics at compile time if any parameter type is (or contains) a `Custom(name)` that
/// is not in `declared`.
pub const fn assert_params_declared(params: &[&idl::Parameter<'_>], declared: &[&str]) {
    let mut i = 0;
    while i < params.len() {
        assert_type_declared(params[i].ty(), declared);
        i += 1;
    }
}

/// Assert at compile time that if a type is a custom type reference, its name appears in
/// the declared types list.
///
/// This is called by generated code from the `#[service]` macro. If the assertion fails,
/// compilation stops with a message telling the user to add the type to `types = [...]`.
///
/// # Panics
///
/// Panics at compile time if `ty` is (or contains) a `Custom(name)` that is not in
/// `declared`.
pub const fn assert_type_declared(ty: &idl::Type<'_>, declared: &[&str]) {
    if let Some(name) = custom_type_name(ty)
        && !name_in_list(name, declared)
    {
        panic!(
            "custom type used in method signature is not declared in `types = [...]`; \
                 add it to the `types` list on the `#[zlink(interface = \"...\", types = [...])]` \
                 attribute"
        );
    }
}

/// Extract the custom type name from an [`idl::Type`], if it is a custom type reference.
///
/// Returns `None` for primitive, optional, array, map, inline enum/object types.
pub const fn custom_type_name<'a>(ty: &'a idl::Type<'a>) -> Option<&'a str> {
    match ty {
        idl::Type::Custom(name) => Some(name),
        idl::Type::Optional(inner) | idl::Type::Array(inner) | idl::Type::Map(inner) => {
            custom_type_name(inner.inner())
        }
        _ => None,
    }
}

// Manual const-compatible string comparison helpers.
//
// `PartialEq` for `[u8]`/`str` is not yet const-stable (see
// https://github.com/rust-lang/rust/issues/143874), so we roll our own
// byte-by-byte comparison. Remove these once const `PartialEq` is stabilized.
const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn str_eq(a: &str, b: &str) -> bool {
    bytes_eq(a.as_bytes(), b.as_bytes())
}

const fn name_in_list(name: &str, declared: &[&str]) -> bool {
    let mut i = 0;
    while i < declared.len() {
        if str_eq(name, declared[i]) {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idl::{Type, TypeRef};

    #[test]
    fn primitive_types_always_pass() {
        // Primitives should never trigger the assertion, even with an empty declared list.
        const _: () = {
            assert_type_declared(&Type::Bool, &[]);
            assert_type_declared(&Type::Int, &[]);
            assert_type_declared(&Type::Float, &[]);
            assert_type_declared(&Type::String, &[]);
            assert_type_declared(&Type::ForeignObject, &[]);
            assert_type_declared(&Type::Any, &[]);
        };
    }

    #[test]
    fn custom_type_passes_when_declared() {
        const _: () = {
            assert_type_declared(&Type::Custom("Book"), &["Book", "Album"]);
            assert_type_declared(&Type::Custom("Album"), &["Book", "Album"]);
        };
    }

    #[test]
    fn nested_custom_type_passes_when_declared() {
        // Optional<Custom>, Array<Custom>, Map<Custom> should all be checked.
        // These can't be const because TypeRef contains a Box variant with Drop.
        static BOOK: Type<'static> = Type::Custom("Book");
        assert_type_declared(&Type::Optional(TypeRef::new(&BOOK)), &["Book"]);
        assert_type_declared(&Type::Array(TypeRef::new(&BOOK)), &["Book"]);
        assert_type_declared(&Type::Map(TypeRef::new(&BOOK)), &["Book"]);
    }

    #[test]
    fn custom_type_name_extracts_correctly() {
        assert!(custom_type_name(&Type::Custom("Book")).is_some());
        assert!(custom_type_name(&Type::Int).is_none());
        assert!(custom_type_name(&Type::String).is_none());

        static BOOK: Type<'static> = Type::Custom("Book");
        assert!(custom_type_name(&Type::Optional(TypeRef::new(&BOOK))).is_some());
        assert!(custom_type_name(&Type::Array(TypeRef::new(&BOOK))).is_some());
    }

    #[test]
    #[should_panic(expected = "not declared in `types = [...]`")]
    fn undeclared_custom_type_panics() {
        assert_type_declared(&Type::Custom("Missing"), &["Book", "Album"]);
    }

    #[test]
    #[should_panic(expected = "not declared in `types = [...]`")]
    fn undeclared_nested_custom_type_panics() {
        static MISSING: Type<'static> = Type::Custom("Missing");
        assert_type_declared(&Type::Optional(TypeRef::new(&MISSING)), &["Book"]);
    }
}
