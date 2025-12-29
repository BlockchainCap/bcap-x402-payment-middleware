use super::{DatabaseError, DatabaseTrait, UserData};
use async_trait::async_trait;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use aws_sdk_dynamodb::Client;

/// DynamoDB implementation of DatabaseTrait
#[derive(Clone)]
pub struct DynamoDbDatabase {
    client: Client,
    table_name: String,
}

impl DynamoDbDatabase {
    /// Create a new DynamoDB database instance
    pub async fn new(table_name: String) -> Result<Self, DatabaseError> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        let client = Client::new(&config);

        tracing::info!(table = %table_name, "DynamoDB client initialized");

        Ok(Self { client, table_name })
    }
}

#[async_trait]
impl DatabaseTrait for DynamoDbDatabase {
    async fn get_user(&self, address: &str) -> Result<Option<UserData>, DatabaseError> {
        let key = address.to_lowercase();

        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("address", AttributeValue::S(key.clone()))
            .send()
            .await
            .map_err(|e| DatabaseError::DynamoDB(e.to_string()))?;

        match result.item {
            Some(item) => {
                let balance = item
                    .get("balance")
                    .and_then(|v| v.as_n().ok())
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or_else(|| DatabaseError::AttributeNotFound("balance".to_string()))?;

                let latest_timestamp = item
                    .get("latest_timestamp")
                    .and_then(|v| v.as_n().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or_else(|| {
                        DatabaseError::AttributeNotFound("latest_timestamp".to_string())
                    })?;

                Ok(Some(UserData::new(balance, latest_timestamp)))
            }
            None => Ok(None),
        }
    }

    async fn update_user(&self, address: &str, data: UserData) -> Result<(), DatabaseError> {
        let key = address.to_lowercase();

        self.client
            .put_item()
            .table_name(&self.table_name)
            .item("address", AttributeValue::S(key.clone()))
            .item("balance", AttributeValue::N(data.balance.to_string()))
            .item(
                "latest_timestamp",
                AttributeValue::N(data.latest_timestamp.to_string()),
            )
            .send()
            .await
            .map_err(|e| DatabaseError::DynamoDB(e.to_string()))?;

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

        // Use atomic update operation
        let result = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("address", AttributeValue::S(key.clone()))
            .update_expression("SET balance = if_not_exists(balance, :zero) + :amount, latest_timestamp = if_not_exists(latest_timestamp, :zero)")
            .expression_attribute_values(":amount", AttributeValue::N(amount.to_string()))
            .expression_attribute_values(":zero", AttributeValue::N("0".to_string()))
            .return_values(ReturnValue::AllNew)
            .send()
            .await
            .map_err(|e| DatabaseError::DynamoDB(e.to_string()))?;

        let new_balance = result
            .attributes
            .and_then(|attrs| attrs.get("balance").cloned())
            .and_then(|v| {
                if let AttributeValue::N(n) = v {
                    n.parse::<f64>().ok()
                } else {
                    None
                }
            })
            .ok_or_else(|| DatabaseError::AttributeNotFound("balance".to_string()))?;

        tracing::info!(
            address = %key,
            added = amount,
            new_balance = new_balance,
            "Balance added"
        );

        Ok(new_balance)
    }

    async fn deduct_balance(
        &self,
        address: &str,
        amount: f64,
        timestamp: u64,
    ) -> Result<f64, DatabaseError> {
        let key = address.to_lowercase();

        // Use atomic update with condition to prevent negative balance
        let result = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("address", AttributeValue::S(key.clone()))
            .update_expression("SET balance = balance - :amount, latest_timestamp = :ts")
            .condition_expression("attribute_exists(balance) AND balance >= :amount")
            .expression_attribute_values(":amount", AttributeValue::N(amount.to_string()))
            .expression_attribute_values(":ts", AttributeValue::N(timestamp.to_string()))
            .return_values(ReturnValue::AllNew)
            .send()
            .await
            .map_err(|e| {
                let error_str = e.to_string();
                if error_str.contains("ConditionalCheckFailedException") {
                    DatabaseError::InsufficientBalance {
                        has: 0.0,
                        need: amount,
                    }
                } else {
                    DatabaseError::DynamoDB(error_str)
                }
            })?;

        let remaining_balance = result
            .attributes
            .and_then(|attrs| attrs.get("balance").cloned())
            .and_then(|v| {
                if let AttributeValue::N(n) = v {
                    n.parse::<f64>().ok()
                } else {
                    None
                }
            })
            .ok_or_else(|| DatabaseError::AttributeNotFound("balance".to_string()))?;

        tracing::debug!(
            address = %key,
            deducted = amount,
            remaining = remaining_balance,
            "Balance deducted"
        );

        Ok(remaining_balance)
    }
}

