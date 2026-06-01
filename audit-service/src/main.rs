use tonic::{transport::Server, Request, Response, Status};

pub mod exchange {
    tonic::include_proto!("exchange");
}

use exchange::audit_service_server::{AuditService, AuditServiceServer};
use exchange::{LogEventRequest, LogEventResponse};

pub struct Audit {
    redis_client: redis::Client,
}

impl Audit {
    pub fn new(redis_client: redis::Client) -> Self {
        Self { redis_client }
    }
}

#[tonic::async_trait]
impl AuditService for Audit {
    async fn log_event(
        &self,
        request: Request<LogEventRequest>,
    ) -> Result<Response<LogEventResponse>, Status> {
        let req = request.into_inner();
        println!("Audit Log received event: [{}] for Order ID: {}", req.event_type, req.order_id);

        let mut conn = self.redis_client.get_tokio_connection().await
            .map_err(|e| Status::internal(format!("Failed to connect to Redis: {}", e)))?;

        let timestamp = chrono::Utc::now().to_rfc3339();

        // Push event into Redis Stream named "audit_stream"
        let _: String = redis::cmd("XADD")
            .arg("audit_stream")
            .arg("*")
            .arg("event_type")
            .arg(&req.event_type)
            .arg("order_id")
            .arg(&req.order_id)
            .arg("payload")
            .arg(&req.payload)
            .arg("timestamp")
            .arg(&timestamp)
            .query_async(&mut conn)
            .await
            .map_err(|e| Status::internal(format!("Redis XADD failed: {}", e)))?;

        Ok(Response::new(LogEventResponse { success: true }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let port = std::env::var("PORT").unwrap_or_else(|_| "50054".to_string());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let addr = format!("0.0.0.0:{}", port).parse()?;

    println!("Audit Service starting up...");
    let client = redis::Client::open(redis_url)?;

    println!("Audit Service connected to Redis.");
    println!("Audit Service running on {}", addr);

    let service = Audit::new(client);

    Server::builder()
        .add_service(AuditServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
