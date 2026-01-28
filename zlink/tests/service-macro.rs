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

            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// A simple service with metadata attributes.
struct MetadataService;

#[zlink::service(
    vendor = "Test Vendor",
    product = "Test Product",
    version = "1.0.0",
    url = "https://example.com"
)]
impl MetadataService {
    #[zlink(interface = "org.example.metadata")]
    async fn ping(&self) {}
}
