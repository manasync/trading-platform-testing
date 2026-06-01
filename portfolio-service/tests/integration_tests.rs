use tonic::{transport::Server, Request, Response, Status};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::net::TcpListener;
use tokio::sync::oneshot;
use sqlx::PgPool;

// Import from the library crate so we use identical types
use portfolio_service::exchange::market_service_server::{MarketService, MarketServiceServer};
use portfolio_service::exchange::audit_service_server::{AuditService, AuditServiceServer};
use portfolio_service::exchange::portfolio_service_server::PortfolioService;
use portfolio_service::exchange::{
    GetPriceRequest, GetPriceResponse, GetPricesRequest, GetPricesResponse,
    GetSymbolsRequest, GetSymbolsResponse, LogEventRequest, LogEventResponse,
    CreateOrderRequest, GetPortfolioRequest,
};

// --- Mock Market Service ---
pub struct MockMarket {
    pub price: Arc<Mutex<f64>>,
}

#[tonic::async_trait]
impl MarketService for MockMarket {
    async fn get_symbols(
        &self,
        _request: Request<GetSymbolsRequest>,
    ) -> Result<Response<GetSymbolsResponse>, Status> {
        Ok(Response::new(GetSymbolsResponse {
            symbols: vec!["AAPL".to_string()],
        }))
    }

    async fn get_price(
        &self,
        _request: Request<GetPriceRequest>,
    ) -> Result<Response<GetPriceResponse>, Status> {
        let price = *self.price.lock().unwrap();
        Ok(Response::new(GetPriceResponse {
            symbol: "AAPL".to_string(),
            price,
            success: true,
            error_message: "".to_string(),
        }))
    }

    async fn get_prices(
        &self,
        _request: Request<GetPricesRequest>,
    ) -> Result<Response<GetPricesResponse>, Status> {
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_string(), *self.price.lock().unwrap());
        Ok(Response::new(GetPricesResponse { prices }))
    }
}

// --- Mock Audit Service ---
pub struct MockAudit {
    pub events: Arc<Mutex<Vec<LogEventRequest>>>,
}

#[tonic::async_trait]
impl AuditService for MockAudit {
    async fn log_event(
        &self,
        request: Request<LogEventRequest>,
    ) -> Result<Response<LogEventResponse>, Status> {
        let req = request.into_inner();
        self.events.lock().unwrap().push(req);
        Ok(Response::new(LogEventResponse { success: true }))
    }
}

// --- Helper to start mock servers on ephemeral ports ---
async fn start_mock_market(price: Arc<Mutex<f64>>) -> (String, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = format!("http://127.0.0.1:{}", port);

    let (tx, rx) = oneshot::channel::<()>();
    let mock = MockMarket { price };

    tokio::spawn(async move {
        Server::builder()
            .add_service(MarketServiceServer::new(mock))
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(
                    tokio::net::TcpListener::from_std(listener).unwrap(),
                ),
                async {
                    rx.await.ok();
                },
            )
            .await
            .unwrap();
    });

    (addr, tx)
}

async fn start_mock_audit(events: Arc<Mutex<Vec<LogEventRequest>>>) -> (String, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = format!("http://127.0.0.1:{}", port);

    let (tx, rx) = oneshot::channel::<()>();
    let mock = MockAudit { events };

    tokio::spawn(async move {
        Server::builder()
            .add_service(AuditServiceServer::new(mock))
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(
                    tokio::net::TcpListener::from_std(listener).unwrap(),
                ),
                async {
                    rx.await.ok();
                },
            )
            .await
            .unwrap();
    });

    (addr, tx)
}

// --- Setup Test Database ---
async fn setup_test_db() -> PgPool {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/exchange".to_string());
    
    let db_pool = PgPool::connect(&db_url).await.expect("Failed to connect to Postgres for integration tests. Make sure Postgres is running.");

    // Clean up tables first
    sqlx::query("DROP TABLE IF EXISTS orders;").execute(&db_pool).await.ok();
    sqlx::query("DROP TABLE IF EXISTS holdings;").execute(&db_pool).await.ok();
    sqlx::query("DROP TABLE IF EXISTS users;").execute(&db_pool).await.ok();

    // Create tables
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            user_id TEXT PRIMARY KEY,
            balance DOUBLE PRECISION NOT NULL
        );"
    )
    .execute(&db_pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS holdings (
            user_id TEXT NOT NULL,
            symbol TEXT NOT NULL,
            quantity DOUBLE PRECISION NOT NULL,
            PRIMARY KEY (user_id, symbol)
        );"
    )
    .execute(&db_pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS orders (
            order_id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            order_type TEXT NOT NULL,
            symbol TEXT NOT NULL,
            quantity DOUBLE PRECISION NOT NULL,
            price DOUBLE PRECISION NOT NULL,
            status TEXT NOT NULL,
            reason TEXT,
            created_at TEXT NOT NULL
        );"
    )
    .execute(&db_pool)
    .await
    .unwrap();

    db_pool
}

#[tokio::test]
async fn test_buy_and_sell_flows() {
    let db_pool = setup_test_db().await;

    let price = Arc::new(Mutex::new(150.0));
    let (market_url, _m_tx) = start_mock_market(price.clone()).await;

    let events = Arc::new(Mutex::new(Vec::new()));
    let (audit_url, _a_tx) = start_mock_audit(events.clone()).await;

    // Instantiate Portfolio Service direct struct (so we can test it)
    let portfolio_service = portfolio_service::Portfolio::new(db_pool.clone(), market_url, audit_url);

    let user_id = "test_user_1";

    // 1. Check default balance is seeded
    let get_port_req = Request::new(GetPortfolioRequest {
        user_id: user_id.to_string(),
    });
    let port_resp = portfolio_service.get_portfolio(get_port_req).await.unwrap().into_inner();
    assert_eq!(port_resp.balance, 10000.0);
    assert!(port_resp.holdings.is_empty());

    // 2. Buy AAPL with sufficient balance
    // 10 units of AAPL @ $150 = $1500
    let buy_req = Request::new(CreateOrderRequest {
        user_id: user_id.to_string(),
        order_type: "BUY".to_string(),
        symbol: "AAPL".to_string(),
        quantity: 10.0,
    });
    let buy_resp = portfolio_service.create_order(buy_req).await.unwrap().into_inner();
    assert_eq!(buy_resp.status, "EXECUTED");
    assert_eq!(buy_resp.price, 150.0);

    // 3. Verify portfolio is updated
    let port_resp_2 = portfolio_service.get_portfolio(Request::new(GetPortfolioRequest {
        user_id: user_id.to_string(),
    })).await.unwrap().into_inner();
    assert_eq!(port_resp_2.balance, 8500.0);
    assert_eq!(*port_resp_2.holdings.get("AAPL").unwrap(), 10.0);

    // 4. Buy AAPL with INSUFFICIENT balance
    // 60 units of AAPL @ $150 = $9000 (balance is $8500)
    let buy_fail_req = Request::new(CreateOrderRequest {
        user_id: user_id.to_string(),
        order_type: "BUY".to_string(),
        symbol: "AAPL".to_string(),
        quantity: 60.0,
    });
    let buy_fail_resp = portfolio_service.create_order(buy_fail_req).await.unwrap().into_inner();
    assert_eq!(buy_fail_resp.status, "REJECTED");
    assert_eq!(buy_fail_resp.reason, "Insufficient balance");

    // Portfolio should remain unchanged
    let port_resp_3 = portfolio_service.get_portfolio(Request::new(GetPortfolioRequest {
        user_id: user_id.to_string(),
    })).await.unwrap().into_inner();
    assert_eq!(port_resp_3.balance, 8500.0);
    assert_eq!(*port_resp_3.holdings.get("AAPL").unwrap(), 10.0);

    // 5. Sell AAPL with sufficient asset
    // Sell 4 units @ $150 = +$600
    let sell_req = Request::new(CreateOrderRequest {
        user_id: user_id.to_string(),
        order_type: "SELL".to_string(),
        symbol: "AAPL".to_string(),
        quantity: 4.0,
    });
    let sell_resp = portfolio_service.create_order(sell_req).await.unwrap().into_inner();
    assert_eq!(sell_resp.status, "EXECUTED");
    assert_eq!(sell_resp.price, 150.0);

    // Portfolio check: balance = 8500 + 600 = 9100. holdings = 10 - 4 = 6
    let port_resp_4 = portfolio_service.get_portfolio(Request::new(GetPortfolioRequest {
        user_id: user_id.to_string(),
    })).await.unwrap().into_inner();
    assert_eq!(port_resp_4.balance, 9100.0);
    assert_eq!(*port_resp_4.holdings.get("AAPL").unwrap(), 6.0);

    // 6. Sell AAPL with INSUFFICIENT asset (have 6, try to sell 10)
    let sell_fail_req = Request::new(CreateOrderRequest {
        user_id: user_id.to_string(),
        order_type: "SELL".to_string(),
        symbol: "AAPL".to_string(),
        quantity: 10.0,
    });
    let sell_fail_resp = portfolio_service.create_order(sell_fail_req).await.unwrap().into_inner();
    assert_eq!(sell_fail_resp.status, "REJECTED");
    assert_eq!(sell_fail_resp.reason, "Insufficient assets");

    // Portfolio unchanged
    let port_resp_5 = portfolio_service.get_portfolio(Request::new(GetPortfolioRequest {
        user_id: user_id.to_string(),
    })).await.unwrap().into_inner();
    assert_eq!(port_resp_5.balance, 9100.0);
    assert_eq!(*port_resp_5.holdings.get("AAPL").unwrap(), 6.0);

    // Check that audit logs were produced
    let recorded_events = events.lock().unwrap();
    assert!(recorded_events.len() >= 5);
}

#[tokio::test]
async fn test_market_service_down() {
    let db_pool = setup_test_db().await;

    // Use an invalid port to simulate market service down
    let market_url = "http://127.0.0.1:59999";
    let events = Arc::new(Mutex::new(Vec::new()));
    let (audit_url, _a_tx) = start_mock_audit(events.clone()).await;

    let portfolio_service = portfolio_service::Portfolio::new(db_pool, market_url.to_string(), audit_url);

    let user_id = "test_user_2";

    let req = Request::new(CreateOrderRequest {
        user_id: user_id.to_string(),
        order_type: "BUY".to_string(),
        symbol: "AAPL".to_string(),
        quantity: 5.0,
    });

    let resp = portfolio_service.create_order(req).await.unwrap().into_inner();
    assert_eq!(resp.status, "REJECTED");
    assert!(resp.reason.contains("Market service unavailable") || resp.reason.contains("price query failed"));
}
