use tonic::transport::Server;
use sqlx::PgPool;

use portfolio_service::Portfolio;
use portfolio_service::exchange::portfolio_service_server::PortfolioServiceServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let port = std::env::var("PORT").unwrap_or_else(|_| "50053".to_string());
    let market_url = std::env::var("MARKET_URL").unwrap_or_else(|_| "http://localhost:50051".to_string());
    let audit_url = std::env::var("AUDIT_URL").unwrap_or_else(|_| "http://localhost:50054".to_string());
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/exchange".to_string());
    let addr = format!("0.0.0.0:{}", port).parse()?;

    println!("Portfolio Service starting up...");

    // Auto-create database if it doesn't exist
    if let Some(pos) = db_url.rfind('/') {
        let (left, right) = db_url.split_at(pos);
        let query_param = if let Some(q_pos) = right.find('?') {
            &right[q_pos..]
        } else {
            ""
        };
        let db_name = if let Some(q_pos) = right.find('?') {
            &right[1..q_pos]
        } else {
            &right[1..]
        };

        let admin_url = format!("{}/postgres{}", left, query_param);
        println!("Verifying database '{}' exists via administrative connection...", db_name);
        
        if let Ok(admin_pool) = PgPool::connect(&admin_url).await {
            let check_query = format!("SELECT 1 FROM pg_database WHERE datname = '{}'", db_name);
            let exists: Option<(i32,)> = sqlx::query_as(&check_query)
                .fetch_optional(&admin_pool)
                .await
                .unwrap_or(None);

            if exists.is_none() {
                println!("Database '{}' does not exist. Creating it now...", db_name);
                let create_query = format!("CREATE DATABASE \"{}\"", db_name);
                if let Err(e) = sqlx::query(&create_query).execute(&admin_pool).await {
                    eprintln!("Warning: Failed to auto-create database '{}': {}", db_name, e);
                } else {
                    println!("Database '{}' created successfully.", db_name);
                }
            }
            admin_pool.close().await;
        }
    }

    // Setup Postgres DB Pool
    let db_pool = PgPool::connect(&db_url).await?;

    // Create tables
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            user_id TEXT PRIMARY KEY,
            balance DOUBLE PRECISION NOT NULL
        );"
    )
    .execute(&db_pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS holdings (
            user_id TEXT NOT NULL,
            symbol TEXT NOT NULL,
            quantity DOUBLE PRECISION NOT NULL,
            PRIMARY KEY (user_id, symbol)
        );"
    )
    .execute(&db_pool)
    .await?;

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
    .await?;

    println!("Portfolio Service DB initialized.");
    println!("Connecting to Market Service at {}", market_url);
    println!("Connecting to Audit Service at {}", audit_url);
    println!("Portfolio Service running on {}", addr);

    let service = Portfolio::new(db_pool, market_url, audit_url);

    Server::builder()
        .add_service(PortfolioServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
