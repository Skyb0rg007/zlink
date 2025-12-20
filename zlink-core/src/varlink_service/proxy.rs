//! Client-side proxy API for the `org.varlink.service` interface.
//!
//! This module provides the [`Proxy`] trait which offers convenient methods to call
//! the standard Varlink service interface methods on any connection.

use crate::proxy;

use super::{Error, Info, InterfaceDescription};

/// Client-side proxy for the `org.varlink.service` interface.
///
/// This trait provides methods to call the standard Varlink service interface
/// methods on a connection.
///
/// # Chaining Calls
///
/// The trait is implemented for both [`crate::Connection`] and [`Chain`], allowing you to
/// chain calls together for efficient batching. Use [`crate::Connection::chain_get_info`] or
/// [`crate::Connection::chain_get_interface_description`] to start a chain.
///
/// ## Owned Data Requirement for Chains
///
/// Chain methods require owned types (`DeserializeOwned`) for reply parameters and errors
/// because the internal buffer may be reused between stream iterations. This limitation may be
/// lifted in the future when Rust supports lending streams.
///
/// [`super::OwnedReply`], [`super::OwnedInfo`], and [`super::OwnedError`] are provided for use
/// with the chain API.
///
/// ## Example
///
/// ```no_run
/// use zlink_core::{
///     Connection,
///     varlink_service::{Chain, OwnedError, OwnedInfo, OwnedReply, Proxy},
/// };
/// use futures_util::{pin_mut, stream::StreamExt};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let mut conn: Connection<zlink_core::connection::socket::impl_for_doc::Socket> = todo!();
/// let chain = conn
///     .chain_get_info()?
///     .get_interface_description("org.example.interface")?
///     .get_info()?;
///
/// // Send the chain and process replies.
/// let replies = chain.send::<OwnedReply, OwnedError>().await?;
/// pin_mut!(replies);
///
/// // Process each reply in the order they were chained.
/// while let Some(result) = replies.next().await {
///     let (reply, _fds) = result?;
///     match reply.unwrap().parameters().unwrap() {
///         OwnedReply::Info(info) => {
///             println!("Service: {} v{} by {}", info.product, info.version, info.vendor);
///             println!("URL: {}", info.url);
///             println!("Interfaces: {:?}", info.interfaces);
///         }
///         OwnedReply::InterfaceDescription(desc) => {
///             // Use as_raw() to get the raw description string.
///             println!("Interface description: {}", desc.as_raw().unwrap());
///         }
///     }
/// }
///
/// // For combining multiple interfaces, create a combined reply enum:
/// use serde::Deserialize;
///
/// #[derive(Debug, Deserialize)]
/// #[serde(untagged)]
/// enum CombinedReply {
///     VarlinkService(OwnedReply),
///     // Add other interface reply types here
///     // OtherInterface(other_interface::OwnedReply),
/// }
///
/// #[derive(Debug, zlink_core::ReplyError)]
/// #[zlink(interface = "org.varlink.service")]
/// enum CombinedError {
///     // Varlink service errors
///     InterfaceNotFound { interface: String },
///     // Add other interface error types here
/// }
///
/// // Then use the combined types for cross-interface chaining.
/// let combined_chain = conn
///     .chain_get_info()?;
///     // .other_interface_method()?;  // Chain calls from other interfaces
///
/// // Specify combined types when sending.
/// let combined_replies = combined_chain.send::<CombinedReply, CombinedError>().await?;
/// pin_mut!(combined_replies);
///
/// while let Some(result) = combined_replies.next().await {
///     let (reply, _fds) = result?;
///     match reply {
///         Ok(reply) => {
///             match reply.parameters().unwrap() {
///                 CombinedReply::VarlinkService(varlink_reply) => match varlink_reply {
///                     OwnedReply::Info(info) => println!("Varlink service info: {:?}", info),
///                     OwnedReply::InterfaceDescription(desc) => {
///                         println!("Varlink interface: {:?}", desc)
///                     }
///                 }
///                 // Handle other interface replies here
///             }
///         }
///         Err(error) => {
///             match error {
///                 CombinedError::InterfaceNotFound { interface } => {
///                     println!("Interface not found: {}", interface);
///                 }
///                 // Handle other interface errors here
///             }
///         }
///     }
/// }
///
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "std")]
#[proxy(
    interface = "org.varlink.service",
    crate = "crate",
    chain_name = "Chain"
)]
pub trait Proxy {
    /// Get information about a Varlink service.
    ///
    /// # Returns
    ///
    /// Two-layer result: outer for connection errors, inner for method errors. On success, contains
    /// service information as [`Info`].
    async fn get_info(&mut self) -> crate::Result<core::result::Result<Info<'_>, Error<'_>>>;

    /// Get the IDL description of an interface.
    ///
    /// # Arguments
    ///
    /// * `interface` - The name of the interface to get the description for.
    ///
    /// # Returns
    ///
    /// Two-layer result: outer for connection errors, inner for method errors. On success, contains
    /// the unparsed interface definition as a [`InterfaceDescription`]. Use
    /// [`InterfaceDescription::parse`] to parse it.
    async fn get_interface_description(
        &mut self,
        interface: &str,
    ) -> crate::Result<core::result::Result<InterfaceDescription<'static>, Error<'_>>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    // Use consolidated mock socket from test_utils.
    use crate::{
        test_utils::mock_socket::MockSocket,
        varlink_service::{OwnedError, OwnedReply},
        Connection,
    };

    #[tokio::test]
    async fn chain_api_creation() -> crate::Result<()> {
        // Test that we can create chains with the new API.
        let responses = [
            r#"{"parameters":{"vendor":"Test","product":"TestProduct","version":"1.0","url":"https://test.com","interfaces":["org.varlink.service"]}}"#,
            r#"{"parameters":{"description":"interface org.varlink.service {}"}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        // Test that we can create the chain APIs.
        let _chain1 = conn.chain_get_info()?;
        let _chain2 = conn.chain_get_interface_description("org.varlink.service")?;

        Ok(())
    }

    #[tokio::test]
    async fn chain_extension_methods() -> crate::Result<()> {
        // Test that we can use chain extension methods.
        let responses = [
            r#"{"parameters":{"vendor":"Test","product":"TestProduct","version":"1.0","url":"https://test.com","interfaces":["org.varlink.service"]}}"#,
            r#"{"parameters":{"description":"interface org.varlink.service {}"}}"#,
            r#"{"parameters":{"vendor":"Test","product":"TestProduct","version":"1.0","url":"https://test.com","interfaces":["org.varlink.service"]}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        // Test that we can chain calls using extension methods and actually read replies.
        let chained = conn
            .chain_get_info()?
            .get_interface_description("org.varlink.service")?
            .get_info()?;

        let replies = chained.send::<OwnedReply, OwnedError>().await?;
        use futures_util::{pin_mut, stream::StreamExt};
        pin_mut!(replies);

        // Read first reply (GetInfo).
        let (first_reply, _fds) = replies.next().await.unwrap()?;
        let first_reply = first_reply.unwrap();
        match first_reply.parameters().unwrap() {
            OwnedReply::Info(info) => {
                assert_eq!(info.vendor, "Test");
                assert_eq!(info.product, "TestProduct");
                assert_eq!(info.version, "1.0");
                assert_eq!(info.url, "https://test.com");
                assert_eq!(info.interfaces, ["org.varlink.service"]);
            }
            _ => panic!("Expected Info reply"),
        }

        // Read second reply (GetInterfaceDescription).
        let (second_reply, _fds) = replies.next().await.unwrap()?;
        let second_reply = second_reply.unwrap();
        match second_reply.parameters().unwrap() {
            OwnedReply::InterfaceDescription(desc) => {
                assert_eq!(desc.as_raw().unwrap(), "interface org.varlink.service {}");
            }
            _ => panic!("Expected InterfaceDescription reply"),
        }

        // Read third reply (GetInfo again).
        let (third_reply, _fds) = replies.next().await.unwrap()?;
        let third_reply = third_reply.unwrap();
        match third_reply.parameters().unwrap() {
            OwnedReply::Info(info) => {
                assert_eq!(info.vendor, "Test");
            }
            _ => panic!("Expected Info reply"),
        }

        // No more replies.
        assert!(replies.next().await.is_none());

        Ok(())
    }
}
