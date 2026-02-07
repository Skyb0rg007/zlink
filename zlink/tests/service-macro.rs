//! Test the `#[service]` attribute macro.

#![cfg(feature = "service")]

use serde::{Deserialize, Serialize};
use zlink::{
    connection::socket::FetchPeerCredentials,
    introspect::{self, CustomType, Type},
    unix::{bind, connect},
    Server,
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_basic() -> Result<(), Box<dyn std::error::Error>> {
    // Remove the socket file if it exists (from a previous run of this test).
    let socket_path = "/tmp/zlink-service-macro-test.sock";
    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

    // Setup the server and run it in a separate task.
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
    let mut conn = connect(socket_path).await?;

    // Test GetBalance method - returns plain value, no Result.
    let reply = conn.get_balance().await?.unwrap();
    assert_eq!(reply.amount, 1000);

    // Test successful Deposit (returns Result<Balance, BankError>).
    let reply = conn.deposit(500).await?.unwrap();
    assert_eq!(reply.amount, 1500);

    // Test GetBalance again to verify state was updated.
    let reply = conn.get_balance().await?.unwrap();
    assert_eq!(reply.amount, 1500);

    // Test successful Withdraw.
    let reply = conn.withdraw(200).await?.unwrap();
    assert_eq!(reply.amount, 1300);

    // Test error: withdraw more than available (InsufficientFunds).
    let err = conn.withdraw(5000).await?.unwrap_err();
    assert_eq!(
        err,
        BankError::InsufficientFunds {
            available: 1300,
            requested: 5000,
        }
    );

    // Verify balance unchanged after failed withdrawal.
    let reply = conn.get_balance().await?.unwrap();
    assert_eq!(reply.amount, 1300);

    // Test error: invalid amount (negative deposit).
    let err = conn.deposit(-100).await?.unwrap_err();
    assert_eq!(err, BankError::InvalidAmount { amount: -100 });

    // Test LockAccount - returns no value (void method).
    conn.lock_account().await?.unwrap();

    // Test error: operations on locked account.
    let err = conn.deposit(100).await?.unwrap_err();
    assert_eq!(err, BankError::AccountLocked);

    let err = conn.withdraw(100).await?.unwrap_err();
    assert_eq!(err, BankError::AccountLocked);

    // GetBalance should still work on locked account.
    let reply = conn.get_balance().await?.unwrap();
    assert_eq!(reply.amount, 1300);

    Ok(())
}

// Response type for balance operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Balance {
    amount: i64,
}

// Error type with parameters - demonstrates error handling.
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.bank")]
enum BankError {
    InsufficientFunds { available: i64, requested: i64 },
    InvalidAmount { amount: i64 },
    AccountLocked,
}

// Define the service type.
struct BankAccount {
    balance: i64,
    locked: bool,
}

impl BankAccount {
    fn new(initial_balance: i64, locked: bool) -> Self {
        Self {
            balance: initial_balance,
            locked,
        }
    }
}

// Apply the service macro.
#[zlink::service(types = [Balance])]
impl BankAccount {
    // Method that returns a plain value (not Result).
    #[zlink(interface = "org.example.bank")]
    async fn get_balance(&self) -> Balance {
        Balance {
            amount: self.balance,
        }
    }

    // Method that can fail - returns Result<Balance, BankError>.
    async fn deposit(&mut self, amount: i64) -> Result<Balance, BankError> {
        if self.locked {
            return Err(BankError::AccountLocked);
        }
        if amount <= 0 {
            return Err(BankError::InvalidAmount { amount });
        }
        self.balance += amount;
        Ok(Balance {
            amount: self.balance,
        })
    }

    // Another method that can fail.
    async fn withdraw(&mut self, amount: i64) -> Result<Balance, BankError> {
        if self.locked {
            return Err(BankError::AccountLocked);
        }
        if amount <= 0 {
            return Err(BankError::InvalidAmount { amount });
        }
        if amount > self.balance {
            return Err(BankError::InsufficientFunds {
                available: self.balance,
                requested: amount,
            });
        }
        self.balance -= amount;
        Ok(Balance {
            amount: self.balance,
        })
    }

    // Method returning Result<(), BankError> (void success, can fail).
    async fn lock_account(&mut self) -> Result<(), BankError> {
        if self.locked {
            return Err(BankError::AccountLocked);
        }
        self.locked = true;
        Ok(())
    }
}

// Define a proxy for the client side.
#[zlink::proxy("org.example.bank")]
trait BankProxy {
    async fn get_balance(&mut self) -> zlink::Result<Result<Balance, BankError>>;
    async fn deposit(&mut self, amount: i64) -> zlink::Result<Result<Balance, BankError>>;
    async fn withdraw(&mut self, amount: i64) -> zlink::Result<Result<Balance, BankError>>;
    async fn lock_account(&mut self) -> zlink::Result<Result<(), BankError>>;
}

// Define a proxy with a non-existent method for testing MethodNotFound error.
#[zlink::proxy("org.example.bank")]
trait UnknownMethodProxy {
    async fn nonexistent_method(&mut self) -> zlink::Result<Result<(), BankError>>;
}

// ============================================================================
// Test custom socket bounds via user-provided generics
// ============================================================================

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_with_custom_socket_bounds() -> Result<(), Box<dyn std::error::Error>> {
    // Remove the socket file if it exists.
    let socket_path = "/tmp/zlink-service-macro-creds-test.sock";
    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

    // Setup the server with the credential-checking service.
    let listener = bind(socket_path).unwrap();
    let service = CredentialCheckingService { balance: 1000 };
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = async {
            let mut conn = connect(socket_path).await?;
            // Test that the service works and can check credentials.
            // The multiplier parameter is used AFTER an await point in the service method,
            // which tests the fix for issue #216 (parameters with #[zlink(connection)]).
            let reply = conn.get_balance_with_creds(2).await?.unwrap();
            assert_eq!(reply.amount, 2000); // 1000 * 2
            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// Error type for credential-checking service.
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.creds")]
enum CredsError {
    CredentialCheckFailed,
}

/// A service that uses custom socket bounds to check peer credentials.
struct CredentialCheckingService {
    balance: i64,
}

// Service implementation with custom socket bounds using user-provided generics.
// The first type parameter (Sock) is used as the socket type. The Socket bound is added
// automatically, so we only specify additional bounds.
#[zlink::service]
impl<Sock> CredentialCheckingService
where
    Sock::ReadHalf: FetchPeerCredentials,
{
    #[zlink(interface = "org.example.creds")]
    async fn get_balance_with_creds(
        &self,
        multiplier: i64,
        #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
    ) -> Result<Balance, CredsError> {
        // Actually check credentials using the connection parameter.
        let creds = conn.peer_credentials().await.unwrap();
        // Verify we got valid credentials (check that unix_user_id is returned).
        let _ = creds.unix_user_id();
        // Use multiplier AFTER the await point - this tests the fix for issue #216.
        // Without `async move`, the multiplier would be captured by reference and not live long
        // enough.
        Ok(Balance {
            amount: self.balance * multiplier,
        })
    }
}

#[zlink::proxy("org.example.creds")]
trait CredsProxy {
    async fn get_balance_with_creds(
        &mut self,
        multiplier: i64,
    ) -> zlink::Result<Result<Balance, CredsError>>;
}

// ============================================================================
// Test service implementing multiple interfaces (each with its own error type)
// ============================================================================

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_multiple_interfaces() -> Result<(), Box<dyn std::error::Error>> {
    // Remove the socket file if it exists.
    let socket_path = "/tmp/zlink-service-macro-multi-iface-test.sock";
    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

    // Setup the server with the multi-interface service.
    let listener = bind(socket_path).unwrap();
    let service = MultiInterfaceService {
        user_authenticated: false,
        items: vec!["apple".to_string(), "banana".to_string()],
    };
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_multi_interface_client(socket_path) => res?,
    }

    Ok(())
}

async fn run_multi_interface_client(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = connect(socket_path).await?;

    // Test org.example.auth interface.

    // Test AuthError: not authenticated.
    let err = conn.get_user_info().await?.unwrap_err();
    assert_eq!(err, AuthError::NotAuthenticated);

    // Test successful authentication.
    conn.authenticate("secret".to_string()).await?.unwrap();

    // Test AuthError: invalid credentials.
    let err = conn.authenticate("wrong".to_string()).await?.unwrap_err();
    assert_eq!(
        err,
        AuthError::InvalidCredentials {
            reason: "wrong password".to_string()
        }
    );

    // After successful auth, get_user_info should work.
    let info = conn.get_user_info().await?.unwrap();
    assert_eq!(info.name, "TestUser");

    // Test org.example.storage interface.

    // Test method returning plain value (no error).
    let count = conn.item_count().await?.unwrap();
    assert_eq!(count.count, 2);

    // Test StorageError: item not found.
    let err = conn.get_item(10).await?.unwrap_err();
    assert_eq!(err, StorageError::NotFound);

    // Test successful item retrieval.
    let item = conn.get_item(0).await?.unwrap();
    assert_eq!(item.value, "apple");

    // Test StorageError: quota exceeded.
    let err = conn.add_item("cherry".to_string()).await?.unwrap_err();
    assert_eq!(err, StorageError::QuotaExceeded { limit: 2 });

    Ok(())
}

/// Response type for item count.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
struct ItemCount {
    count: usize,
}

/// Response type for user info.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
struct UserInfo {
    name: String,
}

/// Response type for item retrieval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
struct Item {
    value: String,
}

/// Authentication-related errors (for org.example.auth interface).
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.auth")]
enum AuthError {
    NotAuthenticated,
    InvalidCredentials { reason: String },
}

/// Storage-related errors (for org.example.storage interface).
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.storage")]
enum StorageError {
    NotFound,
    QuotaExceeded { limit: usize },
}

/// A service that implements multiple interfaces.
struct MultiInterfaceService {
    user_authenticated: bool,
    items: Vec<String>,
}

#[zlink::service]
impl MultiInterfaceService {
    // ---- org.example.auth interface ----

    /// Authenticate with a password.
    #[zlink(interface = "org.example.auth")]
    async fn authenticate(&mut self, password: String) -> Result<(), AuthError> {
        if password == "secret" {
            self.user_authenticated = true;
            Ok(())
        } else {
            Err(AuthError::InvalidCredentials {
                reason: "wrong password".to_string(),
            })
        }
    }

    /// Get user info (requires authentication).
    async fn get_user_info(&self) -> Result<UserInfo, AuthError> {
        if self.user_authenticated {
            Ok(UserInfo {
                name: "TestUser".to_string(),
            })
        } else {
            Err(AuthError::NotAuthenticated)
        }
    }

    // ---- org.example.storage interface ----

    /// Get the number of items (returns plain value, no Result).
    #[zlink(interface = "org.example.storage")]
    async fn item_count(&self) -> ItemCount {
        ItemCount {
            count: self.items.len(),
        }
    }

    /// Get an item by index.
    async fn get_item(&self, index: usize) -> Result<Item, StorageError> {
        self.items
            .get(index)
            .map(|v| Item { value: v.clone() })
            .ok_or(StorageError::NotFound)
    }

    /// Add a new item.
    async fn add_item(&mut self, item: String) -> Result<(), StorageError> {
        if self.items.len() >= 2 {
            Err(StorageError::QuotaExceeded { limit: 2 })
        } else {
            self.items.push(item);
            Ok(())
        }
    }
}

/// Proxy for org.example.auth interface.
#[zlink::proxy("org.example.auth")]
trait AuthProxy {
    async fn authenticate(&mut self, password: String) -> zlink::Result<Result<(), AuthError>>;
    async fn get_user_info(&mut self) -> zlink::Result<Result<UserInfo, AuthError>>;
}

/// Proxy for org.example.storage interface.
#[zlink::proxy("org.example.storage")]
trait StorageProxy {
    async fn item_count(&mut self) -> zlink::Result<Result<ItemCount, StorageError>>;
    async fn get_item(&mut self, index: usize) -> zlink::Result<Result<Item, StorageError>>;
    async fn add_item(&mut self, item: String) -> zlink::Result<Result<(), StorageError>>;
}

// ============================================================================
// Test introspection support (GetInfo and GetInterfaceDescription)
// ============================================================================

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_introspection() -> Result<(), Box<dyn std::error::Error>> {
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
        res = run_introspection_client(socket_path) => res?,
    }

    Ok(())
}

async fn run_introspection_client(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
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

// ============================================================================
// Test service with metadata attributes
// ============================================================================

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_with_metadata() -> Result<(), Box<dyn std::error::Error>> {
    use zlink::varlink_service::Proxy as VarlinkProxy;

    // Remove the socket file if it exists.
    let socket_path = "/tmp/zlink-service-macro-metadata-test.sock";
    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

    // Setup the server with a service that has metadata.
    let listener = bind(socket_path).unwrap();
    let service = MetadataService;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = async {
            let mut conn = connect(socket_path).await?;

            // Test GetInfo - should return service metadata.
            let info = conn.get_info().await?.unwrap();
            assert_eq!(info.vendor, "Test Vendor");
            assert_eq!(info.product, "Test Product");
            assert_eq!(info.version, "1.0.0");
            assert_eq!(info.url, "https://example.com");
            let interfaces: Vec<&str> = info.interfaces.iter().map(|s| s.as_ref()).collect();
            assert_eq!(
                interfaces.as_slice(),
                ["org.example.metadata", "org.varlink.service"],
                "Unexpected interfaces"
            );

            // Test GetInterfaceDescription - verify both methods are exposed.
            // This tests that the macro-level interface attribute applies to all methods.
            let desc = conn.get_interface_description("org.example.metadata").await?.unwrap();
            let interface = desc.parse()?;
            let method_names: Vec<_> = interface.methods().map(|m| m.name()).collect();
            assert_eq!(
                method_names.as_slice(),
                ["Ping", "Pong"],
                "Expected both Ping and Pong methods from macro-level interface attribute"
            );

            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// A simple service with metadata attributes.
/// This is `pub` to test that the generated types work with public service structs (issue #216).
pub struct MetadataService;

// Test the interface attribute at the macro level instead of on each method.
#[zlink::service(
    interface = "org.example.metadata",
    vendor = "Test Vendor",
    product = "Test Product",
    version = "1.0.0",
    url = "https://example.com"
)]
impl MetadataService {
    async fn ping(&self) {}

    // Add another method to verify all methods get the interface.
    async fn pong(&self) {}
}

// ============================================================================
// Test streaming service methods (#[zlink(more)])
// ============================================================================

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_streaming() -> Result<(), Box<dyn std::error::Error>> {
    // Remove the socket file if it exists.
    let socket_path = "/tmp/zlink-service-macro-streaming-test.sock";
    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

    // Setup the server with a streaming service.
    let listener = bind(socket_path).unwrap();
    let service = StreamingService {
        values: vec![10, 20, 30, 40, 50],
    };
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_streaming_client(socket_path) => res?,
    }

    Ok(())
}

async fn run_streaming_client(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::StreamExt;

    let mut conn = connect(socket_path).await?;

    // Test streaming method.
    let mut stream = std::pin::pin!(conn.get_values().await?);

    // Collect all values from the stream.
    let mut values = Vec::new();
    while let Some(result) = stream.next().await {
        let value = result?.unwrap();
        values.push(value.value);
    }

    assert_eq!(values, vec![10, 20, 30, 40, 50]);

    Ok(())
}

/// Response type for streaming values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, introspect::Type)]
struct StreamValue {
    value: i64,
}

/// A service that has a streaming method.
struct StreamingService {
    values: Vec<i64>,
}

#[zlink::service(interface = "org.example.streaming")]
impl StreamingService {
    #[zlink(more)]
    async fn get_values(
        &self,
        more: bool,
    ) -> impl futures_util::Stream<Item = zlink::Reply<StreamValue>> + Unpin {
        // Clone values to create an owned iterator (avoids lifetime issues).
        let values: Vec<StreamValue> = self
            .values
            .iter()
            .map(|&v| StreamValue { value: v })
            .collect();
        // If more=false, only return the first value.
        let values: Vec<StreamValue> = if more {
            values
        } else {
            values.into_iter().take(1).collect()
        };
        // For finite streams, manually set continues flag.
        let n = values.len();
        futures_util::stream::iter(
            values
                .into_iter()
                .enumerate()
                .map(move |(i, v)| zlink::Reply::new(Some(v)).set_continues(Some(i < n - 1))),
        )
    }
}

/// Proxy for streaming service.
#[zlink::proxy("org.example.streaming")]
trait StreamingProxy {
    #[zlink(more)]
    async fn get_values(
        &mut self,
    ) -> zlink::Result<impl futures_util::Stream<Item = zlink::Result<Result<StreamValue, ()>>>>;
}

// ============================================================================
// Test file descriptor passing with service macro
// ============================================================================

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_fd_passing() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = "/tmp/zlink-service-macro-fd-test.sock";
    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

    let listener = bind(socket_path).unwrap();
    let service = FdService;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_fd_client(socket_path) => res?,
    }

    Ok(())
}

async fn run_fd_client(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };

    let mut conn = connect(socket_path).await?;

    // Send multiple FDs and read from a specific one by index.
    let (r0, mut w0) = UnixStream::pair()?;
    let (r1, mut w1) = UnixStream::pair()?;
    let (r2, mut w2) = UnixStream::pair()?;
    w0.write_all(b"data-zero")?;
    w1.write_all(b"data-one")?;
    w2.write_all(b"data-two")?;
    drop((w0, w1, w2));
    let fds = vec![r0.into(), r1.into(), r2.into()];
    // Read from index 1.
    let data = conn.read_fd(1, fds).await?.unwrap();
    assert_eq!(data, "data-one");

    // Invalid index returns an error.
    let (r, mut w) = UnixStream::pair()?;
    w.write_all(b"some data")?;
    drop(w);
    let result = conn.read_fd(5, vec![r.into()]).await?;
    assert!(matches!(result, Err(FdError::InvalidIndex { index: 5 })));

    // Receive FDs from the service. Each handle has a name and fd_index referencing the FD vector.
    let names = vec!["config.txt".into(), "data.bin".into(), "log.txt".into()];
    let (result, fds) = conn.open_fds(names).await?;
    let handles = result.unwrap();
    assert_eq!(handles.len(), 3);
    assert_eq!(fds.len(), 3);
    // Verify each handle's name and that the FD at fd_index contains the name as content.
    for handle in &handles {
        let fd = &fds[handle.fd_index as usize];
        let cloned_fd = fd.try_clone()?;
        let mut stream = UnixStream::from(cloned_fd);
        let mut buf = String::new();
        stream.read_to_string(&mut buf)?;
        assert_eq!(buf, handle.name);
    }

    // Receive zero FDs from the service.
    let (result, fds) = conn.open_fds(Vec::new()).await?;
    let handles = result.unwrap();
    assert!(handles.is_empty());
    assert!(fds.is_empty());

    // Receive an FD on success path and verify the handle's index references the correct FD.
    let (result, fds) = conn.try_open_fd("success.txt".into(), false).await?;
    let handle = result.unwrap();
    assert_eq!(handle.name, "success.txt");
    assert_eq!(handle.fd_index, 0);
    assert_eq!(fds.len(), 1);
    let mut stream = UnixStream::from(fds.into_iter().next().unwrap());
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    assert_eq!(buf, "success.txt");

    // Receive an FD on error path and verify the diagnostic content.
    let (result, fds) = conn.try_open_fd("missing.txt".into(), true).await?;
    let err = result.unwrap_err();
    assert!(matches!(err, FdError::NotFound { name } if name == "missing.txt"));
    assert_eq!(fds.len(), 1);
    let mut stream = UnixStream::from(fds.into_iter().next().unwrap());
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    assert_eq!(buf, "error-diagnostic");

    Ok(())
}

// Response type for FD operations. The `fd_index` field references a position in the FD vector.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FdHandle {
    name: String,
    fd_index: u32,
}

// Error type for FD operations.
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.fd")]
enum FdError {
    InvalidIndex { index: u32 },
    NotFound { name: String },
}

// A service that tests file descriptor passing.
struct FdService;

#[zlink::service(interface = "org.example.fd")]
impl FdService {
    /// Receive FDs and read from the one at the given index.
    async fn read_fd(
        &self,
        fd_index: u32,
        #[zlink(fds)] fds: Vec<std::os::fd::OwnedFd>,
    ) -> Result<String, FdError> {
        use std::{io::Read, os::unix::net::UnixStream};

        let Some(fd) = fds.into_iter().nth(fd_index as usize) else {
            return Err(FdError::InvalidIndex { index: fd_index });
        };
        let mut stream = UnixStream::from(fd);
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        Ok(buf)
    }

    /// Open a list of named FDs and return handles with their indexes.
    #[zlink(return_fds)]
    async fn open_fds(&self, names: Vec<String>) -> (Vec<FdHandle>, Vec<std::os::fd::OwnedFd>) {
        use std::{io::Write, os::unix::net::UnixStream};

        let mut handles = Vec::new();
        let mut fds = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            let (r, mut w) = UnixStream::pair().unwrap();
            // Write the name as the FD content for verification.
            w.write_all(name.as_bytes()).unwrap();
            drop(w);
            handles.push(FdHandle {
                name,
                fd_index: i as u32,
            });
            fds.push(r.into());
        }
        (handles, fds)
    }

    /// Try to open an FD. On success, return the handle with its index. On error, return the
    /// error alongside a diagnostic FD.
    #[zlink(return_fds)]
    async fn try_open_fd(
        &self,
        name: String,
        should_fail: bool,
    ) -> (Result<FdHandle, FdError>, Vec<std::os::fd::OwnedFd>) {
        use std::{io::Write, os::unix::net::UnixStream};

        let (r, mut w) = UnixStream::pair().unwrap();
        if should_fail {
            w.write_all(b"error-diagnostic").unwrap();
            drop(w);
            (
                Err(FdError::NotFound { name }),
                vec![std::os::fd::OwnedFd::from(r)],
            )
        } else {
            w.write_all(name.as_bytes()).unwrap();
            drop(w);
            (
                Ok(FdHandle { name, fd_index: 0 }),
                vec![std::os::fd::OwnedFd::from(r)],
            )
        }
    }
}

// Proxy for FD service.
#[zlink::proxy("org.example.fd")]
trait FdProxy {
    async fn read_fd(
        &mut self,
        fd_index: u32,
        #[zlink(fds)] fds: Vec<std::os::fd::OwnedFd>,
    ) -> zlink::Result<Result<String, FdError>>;

    #[zlink(return_fds)]
    async fn open_fds(
        &mut self,
        names: Vec<String>,
    ) -> zlink::Result<(Result<Vec<FdHandle>, FdError>, Vec<std::os::fd::OwnedFd>)>;

    #[zlink(return_fds)]
    async fn try_open_fd(
        &mut self,
        name: String,
        should_fail: bool,
    ) -> zlink::Result<(Result<FdHandle, FdError>, Vec<std::os::fd::OwnedFd>)>;
}

// ============================================================================
// Test streaming service methods with FD passing (#[zlink(more, return_fds)])
// ============================================================================

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_streaming_with_fds() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = "/tmp/zlink-service-macro-streaming-fd-test.sock";
    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }

    let listener = bind(socket_path).unwrap();
    let service = StreamingFdService;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_streaming_fd_client(socket_path) => res?,
    }

    Ok(())
}

async fn run_streaming_fd_client(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::StreamExt;
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };

    let mut conn = connect(socket_path).await?;

    // =========================================================================
    // Test 1: Stream output FDs (return_fds + more)
    // =========================================================================
    {
        let names = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        let mut stream = std::pin::pin!(conn.stream_fds(names).await?);

        // Collect all stream items.
        let mut handles = Vec::new();
        let mut all_fds = Vec::new();
        while let Some(result) = stream.next().await {
            let (result, fds) = result?;
            let handle = result.unwrap();
            handles.push(handle);
            all_fds.extend(fds);
        }

        // Should have received 3 handles with 3 FDs.
        assert_eq!(handles.len(), 3);
        assert_eq!(all_fds.len(), 3);

        // Verify each handle's FD contains the expected content.
        for (i, handle) in handles.iter().enumerate() {
            assert_eq!(handle.fd_index, i as u32);
            let fd = all_fds[handle.fd_index as usize].try_clone()?;
            let mut stream = UnixStream::from(fd);
            let mut buf = String::new();
            stream.read_to_string(&mut buf)?;
            assert_eq!(buf, handle.name);
        }
    }

    // =========================================================================
    // Test 2: Stream input FDs (fds + more)
    // =========================================================================
    {
        // Create 3 FDs with known content.
        let (r0, mut w0) = UnixStream::pair()?;
        let (r1, mut w1) = UnixStream::pair()?;
        let (r2, mut w2) = UnixStream::pair()?;
        w0.write_all(b"content-zero")?;
        w1.write_all(b"content-one")?;
        w2.write_all(b"content-two")?;
        drop((w0, w1, w2));

        let fds = vec![r0.into(), r1.into(), r2.into()];
        let mut stream = std::pin::pin!(conn.read_fds_streaming(fds).await?);

        // Collect all stream items.
        let mut results = Vec::new();
        while let Some(result) = stream.next().await {
            let read_result = result?.unwrap();
            results.push(read_result);
        }

        // Should have received 3 results.
        assert_eq!(results.len(), 3);

        // Verify each result has the expected content.
        assert_eq!(results[0].fd_index, 0);
        assert_eq!(results[0].content, "content-zero");
        assert_eq!(results[1].fd_index, 1);
        assert_eq!(results[1].content, "content-one");
        assert_eq!(results[2].fd_index, 2);
        assert_eq!(results[2].content, "content-two");
    }

    Ok(())
}

/// Response for streaming FD read operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FdReadResult {
    fd_index: u32,
    content: String,
}

/// A service that streams file descriptors.
struct StreamingFdService;

#[zlink::service(interface = "org.example.streaming_fd")]
impl StreamingFdService {
    /// Stream FDs with handles, one per name. Each stream item contains a handle and the FD.
    #[zlink(more, return_fds)]
    async fn stream_fds(
        &self,
        more: bool,
        names: Vec<String>,
    ) -> impl futures_util::Stream<Item = (zlink::Reply<FdHandle>, Vec<std::os::fd::OwnedFd>)> + Unpin
    {
        use std::{io::Write, os::unix::net::UnixStream};

        // If more=false, only return the first item.
        let names: Vec<String> = if more {
            names
        } else {
            names.into_iter().take(1).collect()
        };

        let n = names.len();
        futures_util::stream::iter(names.into_iter().enumerate().map(move |(i, name)| {
            let (r, mut w) = UnixStream::pair().unwrap();
            w.write_all(name.as_bytes()).unwrap();
            drop(w);
            let handle = FdHandle {
                name,
                fd_index: i as u32,
            };
            let reply = zlink::Reply::new(Some(handle)).set_continues(Some(i < n - 1));
            (reply, vec![r.into()])
        }))
    }

    /// Receive FDs and stream back the content read from each one.
    #[zlink(more)]
    async fn read_fds_streaming(
        &self,
        more: bool,
        #[zlink(fds)] fds: Vec<std::os::fd::OwnedFd>,
    ) -> impl futures_util::Stream<Item = zlink::Reply<FdReadResult>> + Unpin {
        use std::{io::Read, os::unix::net::UnixStream};

        // If more=false, only return the first result.
        let fds: Vec<std::os::fd::OwnedFd> = if more {
            fds
        } else {
            fds.into_iter().take(1).collect()
        };

        let n = fds.len();
        futures_util::stream::iter(fds.into_iter().enumerate().map(move |(i, fd)| {
            let mut stream = UnixStream::from(fd);
            let mut content = String::new();
            stream.read_to_string(&mut content).unwrap();
            let result = FdReadResult {
                fd_index: i as u32,
                content,
            };
            zlink::Reply::new(Some(result)).set_continues(Some(i < n - 1))
        }))
    }
}

/// Proxy for streaming FD service.
#[zlink::proxy("org.example.streaming_fd")]
trait StreamingFdProxy {
    #[zlink(more, return_fds)]
    async fn stream_fds(
        &mut self,
        names: Vec<String>,
    ) -> zlink::Result<
        impl futures_util::Stream<
            Item = zlink::Result<(Result<FdHandle, ()>, Vec<std::os::fd::OwnedFd>)>,
        >,
    >;

    #[zlink(more)]
    async fn read_fds_streaming(
        &mut self,
        #[zlink(fds)] fds: Vec<std::os::fd::OwnedFd>,
    ) -> zlink::Result<impl futures_util::Stream<Item = zlink::Result<Result<FdReadResult, ()>>>>;
}
