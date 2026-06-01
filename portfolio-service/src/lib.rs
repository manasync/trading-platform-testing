use tonic::{Request, Response, Status};
use sqlx::{PgPool, Transaction, Postgres};
use std::collections::HashMap;
use uuid::Uuid;

pub mod exchange {
    tonic::include_proto!("exchange");
}

use exchange::portfolio_service_server::PortfolioService;
use exchange::market_service_client::MarketServiceClient;
use exchange::audit_service_client::AuditServiceClient;
use exchange::{
    GetPortfolioRequest, GetPortfolioResponse, CreateOrderRequest, CreateOrderResponse,
    GetOrderRequest, GetOrderResponse, GetPriceRequest, LogEventRequest,
};

pub struct Portfolio {
    db_pool: PgPool,
    market_url: String,
    audit_url: String,
}

impl Portfolio {
    pub fn new(db_pool: PgPool, market_url: String, audit_url: String) -> Self {
        Self {
            db_pool,
            market_url,
            audit_url,
        }
    }

    async fn log_audit_event(&self, event_type: &str, order_id: &str, payload: &str) {
        match AuditServiceClient::connect(self.audit_url.clone()).await {
            Ok(mut client) => {
                let req = tonic::Request::new(LogEventRequest {
                    event_type: event_type.to_string(),
                    order_id: order_id.to_string(),
                    payload: payload.to_string(),
                });
                if let Err(e) = client.log_event(req).await {
                    eprintln!("Failed to log audit event over gRPC: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Failed to connect to Audit Service for logging: {}", e);
            }
        }
    }

    async fn get_or_create_user_balance(&self, tx: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<f64, sqlx::Error> {
        let row: Option<(f64,)> = sqlx::query_as("SELECT balance FROM users WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?;

        if let Some((balance,)) = row {
            Ok(balance)
        } else {
            let default_balance = 10000.0;
            sqlx::query("INSERT INTO users (user_id, balance) VALUES ($1, $2)")
                .bind(user_id)
                .bind(default_balance)
                .execute(&mut **tx)
                .await?;
            Ok(default_balance)
        }
    }
}

#[tonic::async_trait]
impl PortfolioService for Portfolio {
    async fn get_portfolio(
        &self,
        request: Request<GetPortfolioRequest>,
    ) -> Result<Response<GetPortfolioResponse>, Status> {
        let req = request.into_inner();
        let user_id = req.user_id;

        let mut tx = self.db_pool.begin().await.map_err(|e| Status::internal(e.to_string()))?;
        let balance = self.get_or_create_user_balance(&mut tx, &user_id).await
            .map_err(|e| Status::internal(e.to_string()))?;

        let holdings_rows: Vec<(String, f64)> = sqlx::query_as("SELECT symbol, quantity FROM holdings WHERE user_id = $1")
            .bind(&user_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        tx.commit().await.map_err(|e| Status::internal(e.to_string()))?;

        let mut holdings = HashMap::new();
        for (symbol, qty) in holdings_rows {
            if qty > 0.0 {
                holdings.insert(symbol, qty);
            }
        }

        Ok(Response::new(GetPortfolioResponse {
            user_id,
            balance,
            holdings,
        }))
    }

    async fn create_order(
        &self,
        request: Request<CreateOrderRequest>,
    ) -> Result<Response<CreateOrderResponse>, Status> {
        let req = request.into_inner();
        let user_id = req.user_id;
        let order_type = req.order_type.to_uppercase();
        let symbol = req.symbol.to_uppercase();
        let quantity = req.quantity;

        if quantity <= 0.0 {
            return Err(Status::invalid_argument("Quantity must be greater than 0"));
        }
        if order_type != "BUY" && order_type != "SELL" {
            return Err(Status::invalid_argument("Order type must be BUY or SELL"));
        }

        let order_id = Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();

        // 1. Emit ORDER_CREATED event
        let created_payload = serde_json::json!({
            "user_id": user_id,
            "order_type": order_type,
            "symbol": symbol,
            "quantity": quantity
        }).to_string();
        self.log_audit_event("ORDER_CREATED", &order_id, &created_payload).await;

        // 2. Fetch current price from Market Service
        let mut market_client = match MarketServiceClient::connect(self.market_url.clone()).await {
            Ok(client) => client,
            Err(e) => {
                let reason = format!("Market service unavailable: {}", e);
                let reject_payload = serde_json::json!({ "reason": reason }).to_string();
                self.log_audit_event("ORDER_REJECTED", &order_id, &reject_payload).await;

                // Save rejected order to DB
                let _ = sqlx::query(
                    "INSERT INTO orders (order_id, user_id, order_type, symbol, quantity, price, status, reason, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
                )
                .bind(&order_id)
                .bind(&user_id)
                .bind(&order_type)
                .bind(&symbol)
                .bind(quantity)
                .bind(0.0)
                .bind("REJECTED")
                .bind(&reason)
                .bind(&created_at)
                .execute(&self.db_pool)
                .await;

                return Ok(Response::new(CreateOrderResponse {
                    order_id,
                    user_id,
                    order_type,
                    symbol,
                    quantity,
                    price: 0.0,
                    status: "REJECTED".to_string(),
                    reason,
                    created_at,
                }));
            }
        };

        let price_resp = market_client.get_price(tonic::Request::new(GetPriceRequest {
            symbol: symbol.clone(),
        })).await;

        let price = match price_resp {
            Ok(resp) => resp.into_inner().price,
            Err(e) => {
                let reason = format!("Market service price query failed: {}", e);
                let reject_payload = serde_json::json!({ "reason": reason }).to_string();
                self.log_audit_event("ORDER_REJECTED", &order_id, &reject_payload).await;

                let _ = sqlx::query(
                    "INSERT INTO orders (order_id, user_id, order_type, symbol, quantity, price, status, reason, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
                )
                .bind(&order_id)
                .bind(&user_id)
                .bind(&order_type)
                .bind(&symbol)
                .bind(quantity)
                .bind(0.0)
                .bind("REJECTED")
                .bind(&reason)
                .bind(&created_at)
                .execute(&self.db_pool)
                .await;

                return Ok(Response::new(CreateOrderResponse {
                    order_id,
                    user_id,
                    order_type,
                    symbol,
                    quantity,
                    price: 0.0,
                    status: "REJECTED".to_string(),
                    reason,
                    created_at,
                }));
            }
        };

        // 3. Process the order in a transaction
        let mut tx = self.db_pool.begin().await.map_err(|e| Status::internal(e.to_string()))?;
        let current_balance = self.get_or_create_user_balance(&mut tx, &user_id).await
            .map_err(|e| Status::internal(e.to_string()))?;

        if order_type == "BUY" {
            let total_cost = quantity * price;
            if current_balance >= total_cost {
                // Deduct cash
                let new_balance = current_balance - total_cost;
                sqlx::query("UPDATE users SET balance = $1 WHERE user_id = $2")
                    .bind(new_balance)
                    .bind(&user_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;

                // Add asset
                let holding_qty: Option<(f64,)> = sqlx::query_as("SELECT quantity FROM holdings WHERE user_id = $1 AND symbol = $2")
                    .bind(&user_id)
                    .bind(&symbol)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;

                if let Some((current_qty,)) = holding_qty {
                    sqlx::query("UPDATE holdings SET quantity = $1 WHERE user_id = $2 AND symbol = $3")
                        .bind(current_qty + quantity)
                        .bind(&user_id)
                        .bind(&symbol)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?;
                } else {
                    sqlx::query("INSERT INTO holdings (user_id, symbol, quantity) VALUES ($1, $2, $3)")
                        .bind(&user_id)
                        .bind(&symbol)
                        .bind(quantity)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?;
                }

                // Insert Executed Order
                sqlx::query(
                    "INSERT INTO orders (order_id, user_id, order_type, symbol, quantity, price, status, reason, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
                )
                .bind(&order_id)
                .bind(&user_id)
                .bind(&order_type)
                .bind(&symbol)
                .bind(quantity)
                .bind(price)
                .bind("EXECUTED")
                .bind("")
                .bind(&created_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

                tx.commit().await.map_err(|e| Status::internal(e.to_string()))?;

                let exec_payload = serde_json::json!({ "price": price, "total_cost": total_cost }).to_string();
                self.log_audit_event("ORDER_EXECUTED", &order_id, &exec_payload).await;

                Ok(Response::new(CreateOrderResponse {
                    order_id,
                    user_id,
                    order_type,
                    symbol,
                    quantity,
                    price,
                    status: "EXECUTED".to_string(),
                    reason: "".to_string(),
                    created_at,
                }))
            } else {
                // Insufficient Balance
                sqlx::query(
                    "INSERT INTO orders (order_id, user_id, order_type, symbol, quantity, price, status, reason, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
                )
                .bind(&order_id)
                .bind(&user_id)
                .bind(&order_type)
                .bind(&symbol)
                .bind(quantity)
                .bind(price)
                .bind("REJECTED")
                .bind("Insufficient balance")
                .bind(&created_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

                tx.commit().await.map_err(|e| Status::internal(e.to_string()))?;

                let reason = "Insufficient balance".to_string();
                let reject_payload = serde_json::json!({ "reason": reason, "price": price }).to_string();
                self.log_audit_event("ORDER_REJECTED", &order_id, &reject_payload).await;

                Ok(Response::new(CreateOrderResponse {
                    order_id,
                    user_id,
                    order_type,
                    symbol,
                    quantity,
                    price,
                    status: "REJECTED".to_string(),
                    reason,
                    created_at,
                }))
            }
        } else {
            // SELL Order
            let holding_qty: Option<(f64,)> = sqlx::query_as("SELECT quantity FROM holdings WHERE user_id = $1 AND symbol = $2")
                .bind(&user_id)
                .bind(&symbol)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let current_qty = holding_qty.map(|(q,)| q).unwrap_or(0.0);

            if current_qty >= quantity {
                // Deduct asset
                sqlx::query("UPDATE holdings SET quantity = $1 WHERE user_id = $2 AND symbol = $3")
                    .bind(current_qty - quantity)
                    .bind(&user_id)
                    .bind(&symbol)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;

                // Add cash
                let proceeds = quantity * price;
                let new_balance = current_balance + proceeds;
                sqlx::query("UPDATE users SET balance = $1 WHERE user_id = $2")
                    .bind(new_balance)
                    .bind(&user_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;

                // Insert Executed Order
                sqlx::query(
                    "INSERT INTO orders (order_id, user_id, order_type, symbol, quantity, price, status, reason, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
                )
                .bind(&order_id)
                .bind(&user_id)
                .bind(&order_type)
                .bind(&symbol)
                .bind(quantity)
                .bind(price)
                .bind("EXECUTED")
                .bind("")
                .bind(&created_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

                tx.commit().await.map_err(|e| Status::internal(e.to_string()))?;

                let exec_payload = serde_json::json!({ "price": price, "proceeds": proceeds }).to_string();
                self.log_audit_event("ORDER_EXECUTED", &order_id, &exec_payload).await;

                Ok(Response::new(CreateOrderResponse {
                    order_id,
                    user_id,
                    order_type,
                    symbol,
                    quantity,
                    price,
                    status: "EXECUTED".to_string(),
                    reason: "".to_string(),
                    created_at,
                }))
            } else {
                // Insufficient Assets
                sqlx::query(
                    "INSERT INTO orders (order_id, user_id, order_type, symbol, quantity, price, status, reason, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
                )
                .bind(&order_id)
                .bind(&user_id)
                .bind(&order_type)
                .bind(&symbol)
                .bind(quantity)
                .bind(price)
                .bind("REJECTED")
                .bind("Insufficient assets")
                .bind(&created_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

                tx.commit().await.map_err(|e| Status::internal(e.to_string()))?;

                let reason = "Insufficient assets".to_string();
                let reject_payload = serde_json::json!({ "reason": reason, "price": price }).to_string();
                self.log_audit_event("ORDER_REJECTED", &order_id, &reject_payload).await;

                Ok(Response::new(CreateOrderResponse {
                    order_id,
                    user_id,
                    order_type,
                    symbol,
                    quantity,
                    price,
                    status: "REJECTED".to_string(),
                    reason,
                    created_at,
                }))
            }
        }
    }

    async fn get_order(
        &self,
        request: Request<GetOrderRequest>,
    ) -> Result<Response<GetOrderResponse>, Status> {
        let req = request.into_inner();
        let order_id = req.order_id;

        let row: Option<(String, String, String, f64, f64, String, Option<String>, String)> = sqlx::query_as(
            "SELECT user_id, order_type, symbol, quantity, price, status, reason, created_at FROM orders WHERE order_id = $1"
        )
        .bind(&order_id)
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        if let Some((user_id, order_type, symbol, quantity, price, status, reason, created_at)) = row {
            Ok(Response::new(GetOrderResponse {
                order_id,
                user_id,
                order_type,
                symbol,
                quantity,
                price,
                status,
                reason: reason.unwrap_or_default(),
                created_at,
            }))
        } else {
            Err(Status::not_found(format!("Order {} not found", order_id)))
        }
    }
}
