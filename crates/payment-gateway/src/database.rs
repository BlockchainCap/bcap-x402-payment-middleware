use rocksdb::{DB, Options};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("RocksDB error: {0}")]
    RocksDB(#[from] rocksdb::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Insufficient balance: has {has}, need {need}")]
    InsufficientBalance { has: f64, need: f64 },
}

/// User account data stored in RocksDB
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

/// Database wrapper around RocksDB for persistent user data storage
#[derive(Clone)]
pub struct Database {
    db: Arc<DB>,
}

impl Database {
    /// Open or create a RocksDB database at the specified path
    pub fn open(path: &str) -> Result<Self, DatabaseError> {
        // Create parent directories if they don't exist
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let mut opts = Options::default();
        opts.create_if_missing(true);
        
        let db = DB::open(&opts, path)?;
        
        tracing::info!(path = %path, "RocksDB opened successfully");
        
        Ok(Self { db: Arc::new(db) })
    }

    /// Get user data by address
    pub fn get_user(&self, address: &str) -> Result<Option<UserData>, DatabaseError> {
        let key = address.to_lowercase();
        
        match self.db.get(key.as_bytes())? {
            Some(bytes) => {
                let user_data: UserData = bincode::deserialize(&bytes)?;
                Ok(Some(user_data))
            }
            None => Ok(None),
        }
    }

    /// Update user data
    pub fn update_user(&self, address: &str, data: UserData) -> Result<(), DatabaseError> {
        let key = address.to_lowercase();
        let value = bincode::serialize(&data)?;
        
        self.db.put(key.as_bytes(), value)?;
        
        tracing::debug!(
            address = %key,
            balance = data.balance,
            timestamp = data.latest_timestamp,
            "User data updated"
        );
        
        Ok(())
    }

    /// Add balance to user account (for deposits)
    /// Returns the new balance
    pub fn add_balance(&self, address: &str, amount: f64) -> Result<f64, DatabaseError> {
        let key = address.to_lowercase();
        
        let mut user_data = self.get_user(&key)?.unwrap_or_else(|| {
            UserData::new(0.0, 0)
        });
        
        user_data.balance += amount;
        
        self.update_user(&key, user_data.clone())?;
        
        tracing::info!(
            address = %key,
            added = amount,
            new_balance = user_data.balance,
            "Balance added"
        );
        
        Ok(user_data.balance)
    }

    /// Deduct balance from user account and update timestamp
    /// Returns the remaining balance
    pub fn deduct_balance(
        &self,
        address: &str,
        amount: f64,
        timestamp: u64,
    ) -> Result<f64, DatabaseError> {
        let key = address.to_lowercase();
        
        let mut user_data = self.get_user(&key)?.unwrap_or_else(|| {
            UserData::new(0.0, 0)
        });
        
        if user_data.balance < amount {
            return Err(DatabaseError::InsufficientBalance {
                has: user_data.balance,
                need: amount,
            });
        }
        
        user_data.balance -= amount;
        user_data.latest_timestamp = timestamp;
        
        self.update_user(&key, user_data.clone())?;
        
        tracing::debug!(
            address = %key,
            deducted = amount,
            remaining = user_data.balance,
            "Balance deducted"
        );
        
        Ok(user_data.balance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_operations() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::open(db_path.to_str().unwrap()).unwrap();

        let address = "0x1234567890abcdef1234567890abcdef12345678";

        // Test getting non-existent user
        assert!(db.get_user(address).unwrap().is_none());

        // Test adding balance
        let balance = db.add_balance(address, 10.0).unwrap();
        assert_eq!(balance, 10.0);

        // Test getting user
        let user = db.get_user(address).unwrap().unwrap();
        assert_eq!(user.balance, 10.0);

        // Test deducting balance
        let remaining = db.deduct_balance(address, 3.0, 1234567890).unwrap();
        assert_eq!(remaining, 7.0);

        let user = db.get_user(address).unwrap().unwrap();
        assert_eq!(user.balance, 7.0);
        assert_eq!(user.latest_timestamp, 1234567890);

        // Test insufficient balance
        let result = db.deduct_balance(address, 10.0, 1234567891);
        assert!(result.is_err());
    }
}

