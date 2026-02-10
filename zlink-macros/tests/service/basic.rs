//! Basic service macro tests using a BankAccount example.

use serde::{Deserialize, Serialize};
use zlink::{
    introspect::{self, CustomType},
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
pub(crate) struct Balance {
    pub amount: i64,
}

// Error type with parameters - demonstrates error handling.
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.bank")]
pub(crate) enum BankError {
    InsufficientFunds { available: i64, requested: i64 },
    InvalidAmount { amount: i64 },
    AccountLocked,
}

// Define the service type.
pub(crate) struct BankAccount {
    balance: i64,
    locked: bool,
}

impl BankAccount {
    pub fn new(initial_balance: i64, locked: bool) -> Self {
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
