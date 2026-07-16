use syn::{Attribute, Error, Ident, LitStr};

use crate::utils::skip_unknown_meta;

/// The case convention requested by `#[zlink(rename_all = "...")]`.
///
/// The variants mirror serde's rename rules so the semantics are familiar. Note that the field and
/// variant rules differ, because the source convention differs: fields are `snake_case`, variants
/// are `PascalCase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenameAll {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameAll {
    /// Apply the convention to a struct field name, whose source convention is `snake_case`.
    pub(crate) fn apply_to_field(self, field: &str) -> String {
        match self {
            Self::Lower | Self::Snake => field.to_owned(),
            Self::Upper | Self::ScreamingSnake => field.to_ascii_uppercase(),
            Self::Pascal => {
                let mut pascal = String::new();
                let mut capitalize = true;
                for ch in field.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        pascal.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        pascal.push(ch);
                    }
                }
                pascal
            }
            Self::Camel => {
                let pascal = Self::Pascal.apply_to_field(field);
                match pascal.get(..1) {
                    Some(first) => first.to_ascii_lowercase() + &pascal[1..],
                    None => pascal,
                }
            }
            Self::Kebab => field.replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake.apply_to_field(field).replace('_', "-"),
        }
    }

    /// Apply the convention to an enum variant name, whose source convention is `PascalCase`.
    pub(crate) fn apply_to_variant(self, variant: &str) -> String {
        match self {
            Self::Pascal => variant.to_owned(),
            Self::Lower => variant.to_ascii_lowercase(),
            Self::Upper => variant.to_ascii_uppercase(),
            Self::Camel => match variant.get(..1) {
                Some(first) => first.to_ascii_lowercase() + &variant[1..],
                None => variant.to_owned(),
            },
            Self::Snake => {
                let mut snake = String::new();
                for (i, ch) in variant.char_indices() {
                    if i > 0 && ch.is_uppercase() {
                        snake.push('_');
                    }
                    snake.push(ch.to_ascii_lowercase());
                }
                snake
            }
            Self::ScreamingSnake => Self::Snake.apply_to_variant(variant).to_ascii_uppercase(),
            Self::Kebab => Self::Snake.apply_to_variant(variant).replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake
                .apply_to_variant(variant)
                .replace('_', "-"),
        }
    }

    fn parse(lit: &LitStr) -> Result<Self, Error> {
        let rule = match lit.value().as_str() {
            "lowercase" => Self::Lower,
            "UPPERCASE" => Self::Upper,
            "PascalCase" => Self::Pascal,
            "camelCase" => Self::Camel,
            "snake_case" => Self::Snake,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::Kebab,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            unknown => {
                return Err(Error::new_spanned(
                    lit,
                    format!(
                        "unknown `rename_all` value `{unknown}`, expected one of: {}",
                        VALID_RENAME_ALL.join(", "),
                    ),
                ));
            }
        };

        Ok(rule)
    }
}

/// The name a struct field should carry in the IDL and on the wire.
///
/// `#[zlink(rename)]` wins over an inherited `rename_all`, which wins over the Rust ident.
pub(crate) fn field_name(
    attrs: &[Attribute],
    ident: &Ident,
    rename_all: Option<RenameAll>,
) -> Result<String, Error> {
    resolve(attrs, ident, rename_all, RenameAll::apply_to_field)
}

/// The name an enum variant should carry in the IDL and on the wire.
///
/// `#[zlink(rename)]` wins over an inherited `rename_all`, which wins over the Rust ident.
pub(crate) fn variant_name(
    attrs: &[Attribute],
    ident: &Ident,
    rename_all: Option<RenameAll>,
) -> Result<String, Error> {
    resolve(attrs, ident, rename_all, RenameAll::apply_to_variant)
}

/// Reject a container-level `#[zlink(rename)]` with `msg`, for derives where it means nothing.
///
/// Silently ignoring it would let users ship IDL that does not say what they wrote.
pub(crate) fn reject_container_rename(attrs: &[Attribute], msg: &str) -> Result<(), Error> {
    match parse_rename(attrs)? {
        Some(lit) => Err(Error::new_spanned(lit, msg)),
        None => Ok(()),
    }
}

/// The container's `#[zlink(rename_all = "...")]`, if any.
pub(crate) fn parse_rename_all(attrs: &[Attribute]) -> Result<Option<RenameAll>, Error> {
    match parse_zlink_lit_str(attrs, "rename_all")? {
        Some(lit) => RenameAll::parse(&lit).map(Some),
        None => Ok(None),
    }
}

/// The item's `#[zlink(rename = "...")]`, if any.
pub(crate) fn parse_rename(attrs: &[Attribute]) -> Result<Option<LitStr>, Error> {
    parse_zlink_lit_str(attrs, "rename")
}

fn resolve<F>(
    attrs: &[Attribute],
    ident: &Ident,
    rename_all: Option<RenameAll>,
    apply: F,
) -> Result<String, Error>
where
    F: FnOnce(RenameAll, &str) -> String,
{
    if let Some(lit) = parse_rename(attrs)? {
        return Ok(lit.value());
    }

    let ident = ident.to_string();

    Ok(match rename_all {
        Some(rule) => apply(rule, &ident),
        None => ident,
    })
}

/// The string value of `#[zlink(<key> = "...")]`, if present.
///
/// Unlike `utils::parse_zlink_string_attr` this reports parse errors rather than swallowing them,
/// which is what the rename attributes need in order to reject bad input.
fn parse_zlink_lit_str(attrs: &[Attribute], key: &str) -> Result<Option<LitStr>, Error> {
    let mut result = None;

    for attr in attrs {
        if !attr.path().is_ident("zlink") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident(key) {
                let lit: LitStr = meta.value()?.parse()?;
                if result.is_some() {
                    return Err(meta.error(format!("duplicate `{key}` attribute")));
                }
                result = Some(lit);
            } else {
                skip_unknown_meta(&meta)?;
            }

            Ok(())
        })?;
    }

    Ok(result)
}

const VALID_RENAME_ALL: &[&str] = &[
    "lowercase",
    "UPPERCASE",
    "PascalCase",
    "camelCase",
    "snake_case",
    "SCREAMING_SNAKE_CASE",
    "kebab-case",
    "SCREAMING-KEBAB-CASE",
];

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn field_conventions() {
        let cases = [
            (RenameAll::Lower, "user_name"),
            (RenameAll::Upper, "USER_NAME"),
            (RenameAll::Pascal, "UserName"),
            (RenameAll::Camel, "userName"),
            (RenameAll::Snake, "user_name"),
            (RenameAll::ScreamingSnake, "USER_NAME"),
            (RenameAll::Kebab, "user-name"),
            (RenameAll::ScreamingKebab, "USER-NAME"),
        ];

        for (rule, expected) in cases {
            assert_eq!(rule.apply_to_field("user_name"), expected, "rule: {rule:?}");
        }
    }

    #[test]
    fn variant_conventions() {
        let cases = [
            (RenameAll::Lower, "username"),
            (RenameAll::Upper, "USERNAME"),
            (RenameAll::Pascal, "UserName"),
            (RenameAll::Camel, "userName"),
            (RenameAll::Snake, "user_name"),
            (RenameAll::ScreamingSnake, "USER_NAME"),
            (RenameAll::Kebab, "user-name"),
            (RenameAll::ScreamingKebab, "USER-NAME"),
        ];

        for (rule, expected) in cases {
            assert_eq!(
                rule.apply_to_variant("UserName"),
                expected,
                "rule: {rule:?}"
            );
        }
    }

    #[test]
    fn rename_beats_rename_all() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[zlink(rename = "ID")])];
        let ident: Ident = parse_quote!(user_id);
        let name = field_name(&attrs, &ident, Some(RenameAll::Camel)).unwrap();

        assert_eq!(name, "ID");
    }

    #[test]
    fn rename_all_applies_without_rename() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(user_id);
        let name = field_name(&attrs, &ident, Some(RenameAll::Camel)).unwrap();

        assert_eq!(name, "userId");
    }

    #[test]
    fn ident_used_without_any_attr() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(user_id);

        assert_eq!(field_name(&attrs, &ident, None).unwrap(), "user_id");
    }

    #[test]
    fn rename_all_parsed_alongside_other_keys() {
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[zlink(crate = "crate", rename_all = "camelCase")])];

        assert_eq!(parse_rename_all(&attrs).unwrap(), Some(RenameAll::Camel));
    }

    #[test]
    fn unknown_rename_all_value_rejected() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[zlink(rename_all = "bogus")])];
        let err = parse_rename_all(&attrs).unwrap_err().to_string();

        assert!(
            err.contains("bogus"),
            "message should name the bad value: {err}"
        );
        assert!(
            err.contains("camelCase"),
            "message should list valid values: {err}"
        );
    }

    #[test]
    fn container_rename_rejected_with_message() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[zlink(rename = "Foo")])];
        let err = reject_container_rename(&attrs, "nope")
            .unwrap_err()
            .to_string();

        assert_eq!(err, "nope");
    }
}
