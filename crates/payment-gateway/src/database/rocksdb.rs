use super::{DatabaseError, DatabaseTrait, UserData};
use async_trait::async_trait;
use rocksdb::{Options, DB};
use std::sync::Arc;

/// RocksDB implementation of DatabaseTrait
#[derive(Clone)]
pub struct RocksDbDatabase {
    db: Arc<DB>,
}

impl RocksDbDatabase {
    /// Open or create a RocksDB database at the specified path
    pub fn open(path: &str) -> Result<Self, DatabaseError> {
        // Create parent directories if they don't exist
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let mut opts = Options::default();
        opts.create_if_missing(true);

        let db = DB::open(&opts, path)
            .map_err(|e| DatabaseError::RocksDB(e.to_string()))?;

        tracing::info!(path = %path, "RocksDB opened successfully");

        Ok(Self { db: Arc::new(db) })
    }
}

#[async_trait]
impl DatabaseTrait for RocksDbDatabase {
    async fn get_user(&self, address: &str) -> Result<Option<UserData>, DatabaseError> {
        let key = address.to_lowercase();

        match self.db.get(key.as_bytes())
            .map_err(|e| DatabaseError::RocksDB(e.to_string()))?
        {
            Some(bytes) => {
                let user_data: UserData = bincode::deserialize(&bytes)
                    .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
                Ok(Some(user_data))
            }
            None => Ok(None),
        }
    }

    async fn update_user(&self, address: &str, data: UserData) -> Result<(), DatabaseError> {
        let key = address.to_lowercase();
        let value = bincode::serialize(&data)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        self.db.put(key.as_bytes(), value)
            .map_err(|e| DatabaseError::RocksDB(e.to_string()))?;

        tracing::debug!(
            address = %key,
            balance = data.balance,
            timestamp = data.latest_timestamp,
            "User data updated"
        );

        Ok(())
    }

    async fn add_balance(&self, address: &str, amount: f64) -> Result<f64, DatabaseError> {
        let key = address.to_lowercase();

        let mut user_data = self.get_user(&key).await?.unwrap_or_else(|| {
            UserData::new(0.0, 0)
        });

        user_data.balance += amount;

        self.update_user(&key, user_data.clone()).await?;

        tracing::info!(
            address = %key,
            added = amount,
            new_balance = user_data.balance,
            "Balance added"
        );

        Ok(user_data.balance)
    }

    async fn deduct_balance(
        &self,
        address: &str,
        amount: f64,
        timestamp: u64,
    ) -> Result<f64, DatabaseError> {
        let key = address.to_lowercase();

        let mut user_data = self.get_user(&key).await?.unwrap_or_else(|| {
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

        self.update_user(&key, user_data.clone()).await?;

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

    #[tokio::test]
    async fn test_database_operations() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = RocksDbDatabase::open(db_path.to_str().unwrap()).unwrap();

        let address = "0x1234567890abcdef1234567890abcdef12345678";

        // Test getting non-existent user
        assert!(db.get_user(address).await.unwrap().is_none());

        // Test adding balance
        let balance = db.add_balance(address, 10.0).await.unwrap();
        assert_eq!(balance, 10.0);

        // Test getting user
        let user = db.get_user(address).await.unwrap().unwrap();
        assert_eq!(user.balance, 10.0);

        // Test deducting balance
        let remaining = db.deduct_balance(address, 3.0, 1234567890).await.unwrap();
        assert_eq!(remaining, 7.0);

        let user = db.get_user(address).await.unwrap().unwrap();
        assert_eq!(user.balance, 7.0);
        assert_eq!(user.latest_timestamp, 1234567890);

        // Test insufficient balance
        let result = db.deduct_balance(address, 10.0, 1234567891).await;
        assert!(result.is_err());
    }
}

