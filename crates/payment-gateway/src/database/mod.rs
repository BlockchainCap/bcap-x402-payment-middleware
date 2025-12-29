use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod rocksdb;
pub mod dynamodb;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("RocksDB error: {0}")]
    RocksDB(String),

    #[error("DynamoDB error: {0}")]
    DynamoDB(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Insufficient balance: has {has}, need {need}")]
    InsufficientBalance { has: f64, need: f64 },

    #[error("Attribute not found: {0}")]
    AttributeNotFound(String),

    #[error("Parse error: {0}")]
    ParseError(String),
}

/// User account data stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserData {
    /// Current balance in USDC
    pub balance: f64,
    /// Last successful request timestamp (unix seconds)
    pub latest_timestamp: u64,
}

impl UserData {
    pub fn new(balance: f64, timestamp: u64) -> Self {
        Self {
            balance,
            latest_timestamp: timestamp,
        }
    }
}

/// Database trait for persistent user data storage
#[async_trait]
pub trait DatabaseTrait: Send + Sync {
    /// Get user data by address
    async fn get_user(&self, address: &str) -> Result<Option<UserData>, DatabaseError>;

    /// Update user data
    async fn update_user(&self, address: &str, data: UserData) -> Result<(), DatabaseError>;

    /// Add balance to user account (for deposits)
    /// Returns the new balance
    async fn add_balance(&self, address: &str, amount: f64) -> Result<f64, DatabaseError>;

    /// Deduct balance from user account and update timestamp
    /// Returns the remaining balance
    async fn deduct_balance(
        &self,
        address: &str,
        amount: f64,
        timestamp: u64,
    ) -> Result<f64, DatabaseError>;
}

