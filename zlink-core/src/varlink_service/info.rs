#[cfg(feature = "introspection")]
use crate::introspect::Type;
use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

/// Information about a Varlink service implementation.
///
/// This is the return type for the `GetInfo` method of the `org.varlink.service` interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "introspection", derive(Type))]
#[cfg_attr(feature = "introspection", zlink(crate = "crate"))]
pub struct Info<'a> {
    /// The vendor of the service.
    pub vendor: &'a str,
    /// The product name of the service.
    pub product: &'a str,
    /// The version of the service.
    pub version: &'a str,
    /// The URL associated with the service.
    pub url: &'a str,
    /// List of interfaces provided by the service.
    pub interfaces: Vec<&'a str>,
}

impl<'a> Info<'a> {
    /// Create a new `Info` instance.
    pub fn new(
        vendor: &'a str,
        product: &'a str,
        version: &'a str,
        url: &'a str,
        interfaces: Vec<&'a str>,
    ) -> Self {
        Self {
            vendor,
            product,
            version,
            url,
            interfaces,
        }
    }
}

/// Owned version of [`Info`] for use with the chain API.
///
/// This type uses `String` instead of `&str` for all fields, allowing it to be deserialized
/// as owned data. This is required for the chain API because the internal buffer may be reused
/// between stream iterations.
///
/// # Example
///
/// ```no_run
/// use zlink_core::varlink_service::OwnedInfo;
///
/// // OwnedInfo can be deserialized from JSON without borrowing.
/// let json = r#"{"vendor":"Test","product":"Product","version":"1.0","url":"https://example.com","interfaces":["org.varlink.service"]}"#;
/// let info: OwnedInfo = serde_json::from_str(json).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedInfo {
    /// The vendor of the service.
    pub vendor: String,
    /// The product name of the service.
    pub product: String,
    /// The version of the service.
    pub version: String,
    /// The URL associated with the service.
    pub url: String,
    /// List of interfaces provided by the service.
    pub interfaces: Vec<String>,
}

impl OwnedInfo {
    /// Create a new `OwnedInfo` instance.
    pub fn new(
        vendor: String,
        product: String,
        version: String,
        url: String,
        interfaces: Vec<String>,
    ) -> Self {
        Self {
            vendor,
            product,
            version,
            url,
            interfaces,
        }
    }
}

impl<'a> From<Info<'a>> for OwnedInfo {
    fn from(info: Info<'a>) -> Self {
        Self {
            vendor: info.vendor.into(),
            product: info.product.into(),
            version: info.version.into(),
            url: info.url.into(),
            interfaces: info.interfaces.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization() {
        let mut interfaces = Vec::new();
        interfaces.push("com.example.test");

        let info = Info::new(
            "Test Vendor",
            "Test Product",
            "1.0.0",
            "https://example.com",
            interfaces,
        );

        let json = serde_json::to_string(&info).unwrap();

        assert!(json.contains("Test Vendor"));
        assert!(json.contains("com.example.test"));
    }

    #[test]
    fn deserialization() {
        let json = r#"{
            "vendor": "Test Vendor",
            "product": "Test Product",
            "version": "1.0.0",
            "url": "https://example.com",
            "interfaces": ["com.example.test", "com.example.other"]
        }"#;

        let info: Info<'_> = serde_json::from_str(json).unwrap();

        assert_eq!(info.vendor, "Test Vendor");
        assert_eq!(info.product, "Test Product");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.url, "https://example.com");
        assert_eq!(info.interfaces.len(), 2);
        assert_eq!(info.interfaces[0], "com.example.test");
        assert_eq!(info.interfaces[1], "com.example.other");
    }

    #[test]
    fn round_trip_serialization() {
        let mut interfaces = Vec::new();
        interfaces.push("com.example.test");
        interfaces.push("com.example.other");

        let original = Info::new(
            "Test Vendor",
            "Test Product",
            "1.0.0",
            "https://example.com",
            interfaces,
        );

        // Serialize to JSON
        let json = serde_json::to_string(&original).unwrap();

        // Deserialize back from JSON
        let deserialized: Info<'_> = serde_json::from_str(&json).unwrap();

        // Verify they are equal
        assert_eq!(original, deserialized);
    }
}
