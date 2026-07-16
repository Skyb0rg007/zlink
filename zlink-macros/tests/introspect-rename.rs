#![cfg(feature = "introspection")]

use zlink::{
    idl,
    introspect::{CustomType, Type},
};

#[test]
fn struct_field_rename_all() {
    let idl::Type::Object(fields) = Membership::TYPE else {
        panic!("expected an object type");
    };
    let fields: Vec<_> = fields.iter().collect();

    assert_eq!(fields[0].name(), "userName");
    assert_eq!(fields[1].name(), "groupName");
}

#[test]
fn explicit_rename_overrides_rename_all() {
    let idl::Type::Object(fields) = Overridden::TYPE else {
        panic!("expected an object type");
    };
    let fields: Vec<_> = fields.iter().collect();

    assert_eq!(fields[0].name(), "ID", "explicit rename must win");
    assert_eq!(fields[1].name(), "groupName", "rename_all still applies");
}

#[test]
fn kebab_case_field_names_do_not_break_statics() {
    let idl::Type::Object(fields) = Kebabed::TYPE else {
        panic!("expected an object type");
    };
    let fields: Vec<_> = fields.iter().collect();

    assert_eq!(fields[0].name(), "user-name");
}

#[test]
fn enum_variant_rename_all() {
    let idl::Type::Enum(variants) = Status::TYPE else {
        panic!("expected an enum type");
    };
    let variants: Vec<_> = variants.iter().collect();

    assert_eq!(variants[0].name(), "active");
    assert_eq!(variants[1].name(), "notReady");
    assert_eq!(variants[2].name(), "GONE", "explicit rename must win");
}

#[test]
fn custom_type_container_rename() {
    let idl::CustomType::Object(obj) = Record::CUSTOM_TYPE else {
        panic!("expected an object custom type");
    };

    assert_eq!(obj.name(), "Membership");

    let fields: Vec<_> = obj.fields().collect();
    assert_eq!(fields[0].name(), "userName");
}

#[test]
fn custom_type_rename_keeps_type_reference_in_sync() {
    // The `Type` impl must point at the same name the `CustomType` impl declares, or the IDL
    // reference dangles.
    assert_eq!(Record::TYPE, &idl::Type::Custom("Membership"));
}

#[test]
fn custom_enum_container_rename() {
    let idl::CustomType::Enum(e) = Level::CUSTOM_TYPE else {
        panic!("expected an enum custom type");
    };

    assert_eq!(e.name(), "Severity");

    let variants: Vec<_> = e.variants().collect();
    assert_eq!(variants[0].name(), "low");
}

// `#[allow(unused)]` on test types matches the convention already used throughout
// `tests/introspect-type.rs`: the fields exist only to be described, never read.

#[derive(Type)]
#[allow(unused)]
#[zlink(rename_all = "camelCase")]
struct Membership {
    user_name: String,
    group_name: String,
}

#[derive(Type)]
#[allow(unused)]
#[zlink(rename_all = "camelCase")]
struct Overridden {
    #[zlink(rename = "ID")]
    user_id: String,
    group_name: String,
}

#[derive(Type)]
#[allow(unused)]
#[zlink(rename_all = "kebab-case")]
struct Kebabed {
    user_name: String,
}

#[derive(Type)]
#[allow(unused)]
#[zlink(rename_all = "camelCase")]
enum Status {
    Active,
    NotReady,
    #[zlink(rename = "GONE")]
    Gone,
}

#[derive(CustomType)]
#[allow(unused)]
#[zlink(rename = "Membership", rename_all = "camelCase")]
struct Record {
    user_name: String,
}

#[derive(CustomType)]
#[allow(unused)]
#[zlink(rename = "Severity", rename_all = "lowercase")]
enum Level {
    Low,
    High,
}
