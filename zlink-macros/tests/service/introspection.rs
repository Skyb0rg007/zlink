//! Tests for introspection support (GetInfo and GetInterfaceDescription).

use super::basic::{BankAccount, BankError};
use zlink::{
    unix::{bind, connect},
    Server,
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn introspection() -> Result<(), Box<dyn std::error::Error>> {
    // Remove the socket file if it exists.
    let socket_path = "/tmp/zlink-service-macro-introspection-test.sock";
    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

    // Setup the server with metadata.
    let listener = bind(socket_path).unwrap();
    let service = BankAccount::new(1000, false);
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use zlink::varlink_service::Proxy as VarlinkProxy;

    let mut conn = connect(socket_path).await?;

    // Test GetInfo - should return service info with interfaces.
    let info = conn.get_info().await?.unwrap();
    // Should have exactly the user interface and org.varlink.service.
    let interfaces: Vec<&str> = info.interfaces.iter().map(|s| s.as_ref()).collect();
    assert_eq!(
        interfaces.as_slice(),
        ["org.example.bank", "org.varlink.service"],
        "Unexpected interfaces"
    );

    // Test GetInterfaceDescription for user interface.
    let desc = conn
        .get_interface_description("org.example.bank")
        .await?
        .unwrap();
    // Parse the interface and verify the name.
    let interface = desc.parse()?;
    assert_eq!(
        interface.name(),
        "org.example.bank",
        "Expected org.example.bank interface"
    );

    // Verify the interface contains exactly the expected methods.
    let method_names: Vec<_> = interface.methods().map(|m| m.name()).collect();
    assert_eq!(
        method_names.as_slice(),
        ["GetBalance", "Deposit", "Withdraw", "LockAccount"],
        "Unexpected methods"
    );

    // Verify the interface contains exactly the expected errors.
    let error_names: Vec<_> = interface.errors().map(|e| e.name()).collect();
    assert_eq!(
        error_names.as_slice(),
        ["InsufficientFunds", "InvalidAmount", "AccountLocked"],
        "Unexpected errors"
    );

    // Verify the interface contains exactly the expected custom types.
    let type_names: Vec<_> = interface.custom_types().map(|t| t.name()).collect();
    assert_eq!(
        type_names.as_slice(),
        ["Balance"],
        "Unexpected custom types"
    );

    // Test GetInterfaceDescription for org.varlink.service.
    let desc = conn
        .get_interface_description("org.varlink.service")
        .await?
        .unwrap();
    let interface = desc.parse()?;
    assert_eq!(
        interface.name(),
        "org.varlink.service",
        "Expected org.varlink.service interface"
    );
    // Verify org.varlink.service has exactly GetInfo and GetInterfaceDescription methods.
    let method_names: Vec<_> = interface.methods().map(|m| m.name()).collect();
    assert_eq!(
        method_names.as_slice(),
        ["GetInfo", "GetInterfaceDescription"],
        "Unexpected methods in org.varlink.service"
    );

    // Test InterfaceNotFound error - verify the service returns an error for unknown interface.
    let result = conn
        .get_interface_description("org.example.nonexistent")
        .await;

    match result {
        Err(zlink::Error::VarlinkService(err)) => {
            // Verify it's the correct error type.
            match err.inner() {
                zlink::varlink_service::Error::InterfaceNotFound { interface } => {
                    assert_eq!(interface.as_ref(), "org.example.nonexistent");
                }
                other => panic!("Expected InterfaceNotFound error, got: {other:?}"),
            }
        }
        Ok(Ok(_)) => panic!("Expected error for unknown interface, but got success"),
        Ok(Err(err)) => {
            panic!("Expected VarlinkService error in outer Result, got method error: {err:?}")
        }
        Err(err) => panic!("Expected VarlinkService error, got: {err:?}"),
    }

    // Test MethodNotFound error - call a non-existent method.
    // Note: The method name is reported as "unknown" because serde's `#[serde(other)]`
    // attribute captures unknown variants but doesn't preserve the actual tag value.
    let result = conn.nonexistent_method().await;
    match result {
        Err(zlink::Error::VarlinkService(err)) => match err.inner() {
            zlink::varlink_service::Error::MethodNotFound { method } => {
                // The method name is "unknown" because the generated code uses #[serde(other)].
                assert_eq!(method.as_ref(), "unknown");
            }
            other => panic!("Expected MethodNotFound error, got: {other:?}"),
        },
        Ok(Ok(_)) => panic!("Expected error for unknown method, but got success"),
        Ok(Err(err)) => {
            panic!("Expected VarlinkService error in outer Result, got method error: {err:?}")
        }
        Err(err) => panic!("Expected VarlinkService error, got: {err:?}"),
    }

    Ok(())
}

// Define a proxy with a non-existent method for testing MethodNotFound error.
#[zlink::proxy("org.example.bank")]
trait UnknownMethodProxy {
    async fn nonexistent_method(&mut self) -> zlink::Result<Result<(), BankError>>;
}
