#![cfg(feature = "introspection")]

use serde_json::json;
use zlink::{ReplyError, introspect};

// `zlink::ReplyError` (serde) and `zlink::introspect::ReplyError` (IDL) share a name, so import the
// containing module for the latter rather than aliasing, and reach the trait's associated const
// through a qualified path below.
#[derive(Debug, PartialEq, ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.Test")]
#[zlink(rename_all = "SCREAMING_SNAKE_CASE")]
enum RenamedError {
    NotFound,
    #[zlink(rename = "TimedOut")]
    Timeout {
        #[zlink(rename = "timeoutSeconds")]
        seconds: u32,
    },
    #[zlink(rename_all = "camelCase")]
    BadRequest {
        request_id: u32,
    },
}

#[test]
fn variant_rename_all_reaches_the_wire() {
    let json = serde_json::to_value(RenamedError::NotFound).unwrap();

    assert_eq!(json, json!({"error": "org.example.Test.NOT_FOUND"}));
}

#[test]
fn explicit_variant_rename_beats_rename_all() {
    let error = RenamedError::Timeout { seconds: 30 };
    let json = serde_json::to_value(&error).unwrap();

    assert_eq!(
        json,
        json!({
            "error": "org.example.Test.TimedOut",
            "parameters": {"timeoutSeconds": 30},
        }),
    );
}

#[test]
fn variant_rename_all_applies_to_that_variants_fields() {
    let error = RenamedError::BadRequest { request_id: 7 };
    let json = serde_json::to_value(&error).unwrap();

    assert_eq!(
        json,
        json!({
            "error": "org.example.Test.BAD_REQUEST",
            "parameters": {"requestId": 7},
        }),
    );
}

#[test]
fn renamed_variants_round_trip() {
    // `Timeout` covers the explicit-rename path (variant and field renamed via
    // `#[zlink(rename)]`).
    let error = RenamedError::Timeout { seconds: 30 };
    let json = serde_json::to_string(&error).unwrap();
    let back: RenamedError = serde_json::from_str(&json).unwrap();

    assert_eq!(back, error);

    // `BadRequest` covers the `rename_all`-derived path (variant name from the enum-level
    // `SCREAMING_SNAKE_CASE` rule, field name from the variant-level `camelCase` rule).
    let error = RenamedError::BadRequest { request_id: 7 };
    let json = serde_json::to_string(&error).unwrap();
    let back: RenamedError = serde_json::from_str(&json).unwrap();

    assert_eq!(back, error);
}

#[test]
fn idl_error_names_match_the_wire() {
    // The whole point of sharing `naming`: these must not drift. The IDL name is unqualified; the
    // wire name is the same string qualified by the interface.
    let variants = <RenamedError as introspect::ReplyError>::VARIANTS;

    assert_eq!(variants[0].name(), "NOT_FOUND");
    assert_eq!(variants[1].name(), "TimedOut");
    assert_eq!(variants[2].name(), "BAD_REQUEST");
}

#[test]
fn idl_field_names_match_the_wire() {
    let variants = <RenamedError as introspect::ReplyError>::VARIANTS;

    let timeout: Vec<_> = variants[1].fields().collect();
    assert_eq!(timeout[0].name(), "timeoutSeconds");

    let bad_request: Vec<_> = variants[2].fields().collect();
    assert_eq!(bad_request[0].name(), "requestId");
}
