//! Parsers for Varlink IDL using winnow.
//!
//! This module provides parsers for converting IDL strings into the corresponding
//! Rust types defined in the parent module. Uses byte-based parsing to avoid UTF-8 overhead.

use winnow::{
    ModalResult, Parser,
    ascii::multispace0,
    combinator::{alt, delimited, opt, preceded, separated},
    error::{ErrMode, InputError, ParserError},
    token::{literal, take_while},
};

use super::{
    Comment, CustomEnum, CustomObject, CustomType, EnumVariant, Error, Field, Interface, List,
    Method, Parameter, Type, TypeRef,
};

use alloc::{format, vec::Vec};

/// Parse whitespace and comments according to Varlink grammar.
/// The `_` production in Varlink grammar: whitespace / comment / eol_r
fn ws<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    loop {
        let start_len = input.len();

        // Consume regular whitespace (spaces, tabs, etc.)
        multispace0::<_, InputError<&'a [u8]>>
            .parse_next(input)
            .ok();

        // Try to consume a comment: "#" [^\n\r\u{2028}\u{2029}]* eol_r
        if input.starts_with(b"#") {
            // Skip the '#'
            *input = &input[1..];

            // Consume everything until end of line
            while !input.is_empty() {
                match input[0] {
                    b'\n' | b'\r' => {
                        // Consume the end-of-line character(s)
                        if input.starts_with(b"\r\n") {
                            *input = &input[2..];
                        } else {
                            *input = &input[1..];
                        }
                        break;
                    }
                    _ => {
                        *input = &input[1..];
                    }
                }
            }
        }

        // If we didn't consume anything in this iteration, break
        if input.len() == start_len {
            break;
        }
    }
    Ok(())
}

/// Parse only whitespace (not comments) - used in interface parsing where comments are members.
fn whitespace_only<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    multispace0::<_, InputError<&'a [u8]>>
        .parse_next(input)
        .ok();
    Ok(())
}

/// Convert bytes to str with input lifetime.
fn bytes_to_str(bytes: &[u8]) -> &str {
    // SAFETY: We only accept ASCII characters in our parsers
    core::str::from_utf8(bytes).unwrap()
}

/// Parse a field name: starts with letter, continues with alphanumeric and underscores.
fn field_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    let start = *input;
    let mut pos = 0;

    // First character must be alphabetic
    if pos >= input.len() || !input[pos].is_ascii_alphabetic() {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }
    pos += 1;

    // Continue with alphanumeric and underscores
    while pos < input.len() && (input[pos].is_ascii_alphanumeric() || input[pos] == b'_') {
        pos += 1;
    }

    let name_bytes = &start[0..pos];
    *input = &input[pos..];
    Ok(bytes_to_str(name_bytes))
}

/// Parse a type name: starts with uppercase letter, continues with alphanumeric.
fn type_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    let start = *input;
    if input.is_empty() || !input[0].is_ascii_uppercase() {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }

    let mut end = 1;
    while end < input.len() && input[end].is_ascii_alphanumeric() {
        end += 1;
    }

    let name_bytes = &start[0..end];
    *input = &input[end..];
    Ok(bytes_to_str(name_bytes))
}

/// Parse a primitive type.
fn primitive_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((
        literal("bool").map(|_| Type::Bool),
        literal("int").map(|_| Type::Int),
        literal("float").map(|_| Type::Float),
        literal("string").map(|_| Type::String),
        literal("object").map(|_| Type::ForeignObject),
        literal("any").map(|_| Type::Any),
    ))
    .parse_next(input)
}

/// Parse a field in a struct or parameter list.
fn field<'a>(input: &mut &'a [u8]) -> ModalResult<Field<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    let name = field_name(input)?;
    ws(input)?;
    literal(":").parse_next(input)?;
    ws(input)?;
    let ty = varlink_type(input)?;
    Ok(Field::new_owned(name, ty, comments))
}

/// Separator between fields/variants of a comma-separated list.
///
/// `ws` before the comma consumes any whitespace and trailing comments
/// that follow the previous item (e.g. `x: bool # trailing`). After the
/// comma, `whitespace_only` consumes only whitespace, leaving any comments
/// in the input for the next item's `parse_preceding_comments` to attach.
fn field_separator<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    (ws, literal(","), whitespace_only).void().parse_next(input)
}

/// Parse an inline struct type: (field1: type1, field2: type2).
fn struct_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    delimited(
        // Leading whitespace is consumed here so empty/whitespace-only structs
        // parse without being mistaken for a field.
        (literal("("), whitespace_only),
        separated(0.., field, field_separator),
        // `ws` (not `whitespace_only`) so a final comment before `)` (e.g.
        // `(# comment\n)`) is consumed.
        (ws, literal(")")),
    )
    .map(|fields: Vec<Field<'a>>| Type::Object(List::from(fields)))
    .parse_next(input)
}

/// Parse an enum variant: any preceding comments followed by the variant name.
fn enum_variant<'a>(input: &mut &'a [u8]) -> ModalResult<EnumVariant<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;
    let name = field_name(input)?;
    Ok(EnumVariant::new_owned(name, comments))
}

/// Parse an inline enum type: (variant1, variant2, variant3).
fn enum_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    delimited(
        (literal("("), whitespace_only),
        separated(0.., enum_variant, field_separator),
        (ws, literal(")")),
    )
    .map(|variants: Vec<EnumVariant<'a>>| Type::Enum(List::from(variants)))
    .parse_next(input)
}

/// Parse an inline type (struct or enum).
///
/// `struct_type` is tried first: it accepts `()` (and any whitespace- or
/// comment-only body) as an empty struct, per the Varlink grammar
/// (`struct = "(" struct_fields ")" | "(" ")"` — an empty enum cannot be
/// instantiated). Content with `:` matches `struct_type`; bare-name
/// content falls through to `enum_type` via `alt`'s backtracking.
fn inline_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((struct_type, enum_type)).parse_next(input)
}

/// Parse an element type (primitive, custom, or inline).
fn element_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((primitive_type, type_name.map(Type::Custom), inline_type)).parse_next(input)
}

/// Parse an optional type: ?type.
fn optional_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    literal("?").parse_next(input)?;
    let inner = non_optional_type(input)?;
    Ok(Type::Optional(TypeRef::new_owned(inner)))
}

/// Parse any type except optional (to avoid recursion).
fn non_optional_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((array_type, map_type, element_type)).parse_next(input)
}

/// Parse an array type: []type.
fn array_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    literal("[]").parse_next(input)?;
    let inner = varlink_type(input)?;
    Ok(Type::Array(TypeRef::new_owned(inner)))
}

/// Parse a map type: [string]type.
fn map_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    literal("[string]").parse_next(input)?;
    let inner = varlink_type(input)?;
    Ok(Type::Map(TypeRef::new_owned(inner)))
}

/// Parse any Varlink type.
fn varlink_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((optional_type, array_type, map_type, element_type)).parse_next(input)
}

/// Parse an interface name: reverse domain notation like org.example.test.
fn interface_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    let start = *input;
    let mut pos = 0;

    // First segment: [A-Za-z]([-]*[A-Za-z0-9])*
    if pos >= input.len() || !input[pos].is_ascii_alphabetic() {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }
    pos += 1;

    while pos < input.len() && (input[pos].is_ascii_alphanumeric() || input[pos] == b'-') {
        pos += 1;
    }

    let mut found_dot = false;
    // Subsequent segments: .[A-Za-z0-9]([-]*[A-Za-z0-9])*
    while pos < input.len() && input[pos] == b'.' {
        found_dot = true;
        pos += 1; // skip dot

        // Must have at least one alphanumeric after dot
        if pos >= input.len() || !input[pos].is_ascii_alphanumeric() {
            break;
        }
        pos += 1;

        // Continue with alphanumeric and dashes
        while pos < input.len() && (input[pos].is_ascii_alphanumeric() || input[pos] == b'-') {
            pos += 1;
        }
    }

    // Check for at least one dot
    if !found_dot {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }

    let name_bytes = &start[0..pos];
    *input = &input[pos..];
    Ok(bytes_to_str(name_bytes))
}

/// Parse a parameter list: (param1: type1, param2: type2).
fn parameter_list<'a>(
    input: &mut &'a [u8],
) -> ModalResult<Vec<Parameter<'a>>, InputError<&'a [u8]>> {
    delimited(
        (literal("("), whitespace_only),
        separated(0.., field, field_separator),
        (ws, literal(")")),
    )
    .parse_next(input)
}

/// Parse a method definition: method Name(inputs) -> (outputs).
fn method_def<'a>(input: &mut &'a [u8]) -> ModalResult<Method<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("method").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = type_name(input)?;
    ws(input)?;
    let input_params = parameter_list(input)?;
    ws(input)?;
    literal("->").parse_next(input)?;
    ws(input)?;
    let output_params = parameter_list(input)?;

    Ok(Method::new_owned(
        name,
        input_params,
        output_params,
        comments,
    ))
}

/// Parse an error definition: error Name (fields).
fn error_def<'a>(input: &mut &'a [u8]) -> ModalResult<Error<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("error").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = type_name(input)?;
    ws(input)?;
    let params = parameter_list(input)?;

    Ok(Error::new_owned(name, params, comments))
}

/// A typed field or an untyped enum variant. Custom types may be either a
/// struct (all items typed) or an enum (all items untyped); used inside
/// `type_def` to parse both forms uniformly and decide afterwards.
enum FieldOrVariant<'a> {
    Field(Field<'a>),
    Variant(EnumVariant<'a>),
}

/// Parse a single member of a `type Name (...)` body — either `name: type`
/// (a struct field) or `name` (an enum variant).
fn field_or_variant<'a>(
    input: &mut &'a [u8],
) -> ModalResult<FieldOrVariant<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;
    let name = field_name(input)?;
    let ty = opt(preceded((ws, literal(":"), ws), varlink_type)).parse_next(input)?;
    Ok(match ty {
        Some(ty) => FieldOrVariant::Field(Field::new_owned(name, ty, comments)),
        None => FieldOrVariant::Variant(EnumVariant::new_owned(name, comments)),
    })
}

/// Parse a type definition: type Name <definition>.
fn type_def<'a>(input: &mut &'a [u8]) -> ModalResult<CustomType<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("type").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = type_name(input)?;
    ws(input)?;

    let items: Vec<FieldOrVariant<'a>> = delimited(
        (literal("("), whitespace_only),
        separated(0.., field_or_variant, field_separator),
        (ws, literal(")")),
    )
    .parse_next(input)?;

    let mut fields = Vec::new();
    let mut variants = Vec::new();
    for item in items {
        match item {
            FieldOrVariant::Field(f) => fields.push(f),
            FieldOrVariant::Variant(v) => variants.push(v),
        }
    }

    // A custom type cannot mix typed fields and untyped variants.
    if !fields.is_empty() && !variants.is_empty() {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }

    // An empty body is a struct per the Varlink grammar; otherwise, untyped
    // members make an enum and typed members make a struct.
    if !variants.is_empty() {
        Ok(CustomType::from(CustomEnum::new_owned(
            name, variants, comments,
        )))
    } else {
        Ok(CustomType::from(CustomObject::new_owned(
            name, fields, comments,
        )))
    }
}

/// Parse a member definition (type, method, or error).
/// Helper function to parse any preceding comments.
fn parse_preceding_comments<'a>(
    input: &mut &'a [u8],
) -> ModalResult<Vec<Comment<'a>>, InputError<&'a [u8]>> {
    let mut comments = Vec::new();
    while !input.is_empty() {
        let checkpoint = *input;
        whitespace_only(input)?;

        if input.is_empty() {
            break;
        }

        if let Ok(comment) = comment_def(input) {
            comments.push(comment);
            whitespace_only(input)?;
        } else {
            // Not a comment, restore position
            *input = checkpoint;
            break;
        }
    }
    Ok(comments)
}

fn comment_def<'a>(input: &mut &'a [u8]) -> ModalResult<Comment<'a>, InputError<&'a [u8]>> {
    literal("#").parse_next(input)?;

    // Skip all leading whitespace after #
    while !input.is_empty() && (input[0] == b' ' || input[0] == b'\t') {
        *input = &input[1..];
    }

    // Take until newline or end of input - this is the actual comment content
    let line_content = take_while(0.., |c: u8| c != b'\n').parse_next(input)?;
    let comment_text = bytes_to_str(line_content);

    Ok(Comment::new(comment_text))
}

/// Parse an interface definition.
fn interface_def<'a>(input: &mut &'a [u8]) -> ModalResult<Interface<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("interface").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = interface_name(input)?;
    whitespace_only(input)?;

    // Parse members separated by whitespace/newlines
    let mut methods = Vec::new();
    let mut custom_types = Vec::new();
    let mut errors = Vec::new();

    while !input.is_empty() {
        whitespace_only(input)?;

        if input.is_empty() {
            break;
        }

        enum ParsedMember<'a> {
            Custom(CustomType<'a>),
            Method(Method<'a>),
            Error(Error<'a>),
        }

        let result = alt((
            type_def.map(ParsedMember::Custom),
            method_def.map(ParsedMember::Method),
            error_def.map(ParsedMember::Error),
        ))
        .parse_next(input);

        match result {
            Ok(ParsedMember::Custom(custom_type)) => custom_types.push(custom_type),
            Ok(ParsedMember::Method(method)) => methods.push(method),
            Ok(ParsedMember::Error(error)) => errors.push(error),
            Err(_) => break,
        }
    }

    Ok(Interface::new_owned(
        name,
        methods,
        custom_types,
        errors,
        comments,
    ))
}

/// Parse an interface from a string.
pub(super) fn parse_interface(input: &str) -> Result<Interface<'_>, crate::Error> {
    parse_from_str(input, interface_def)
}

/// Helper function to parse from string using byte-based parsers.
fn parse_from_str<'a, T>(
    input: &'a str,
    parser: impl Fn(&mut &'a [u8]) -> ModalResult<T, InputError<&'a [u8]>>,
) -> Result<T, crate::Error> {
    use alloc::string::ToString;

    let input_bytes = input.trim().as_bytes();
    if input_bytes.is_empty() {
        return Err(crate::Error::IdlParse("Input is empty".to_string()));
    }

    let mut input_mut = input_bytes;
    match parser(&mut input_mut) {
        Ok(result) => {
            let _ = ws(&mut input_mut);
            if input_mut.is_empty() {
                Ok(result)
            } else {
                Err(crate::Error::IdlParse(format!(
                    "Unexpected remaining input: {:?}",
                    core::str::from_utf8(input_mut).map_or("<invalid UTF-8>", |s| s)
                )))
            }
        }
        Err(err) => Err(crate::Error::IdlParse(format!("Parse error: {err}"))),
    }
}

#[cfg(test)]
mod tests;
