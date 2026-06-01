use tonic::{transport::Server, Request, Response, Status};
use serde::Deserialize;
use reqwest::Client;

pub mod exchange {
    tonic::include_proto!("exchange");
}

use exchange::market_integration_service_server::{MarketIntegrationService, MarketIntegrationServiceServer};
use exchange::{GetPriceRequest, GetPriceResponse};

#[derive(Debug, Deserialize)]
struct FinnhubQuote {
    c: f64,
}

pub struct MarketIntegration {
    client: Client,
    api_key: String,
}

impl MarketIntegration {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }
}

#[tonic::async_trait]
impl MarketIntegrationService for MarketIntegration {
    async fn get_finnhub_price(
        &self,
        request: Request<GetPriceRequest>,
    ) -> Result<Response<GetPriceResponse>, Status> {
        let req = request.into_inner();
        let symbol = req.symbol.to_uppercase();

        if self.api_key.is_empty() {
            return Ok(Response::new(GetPriceResponse {
                symbol,
                price: 0.0,
                success: false,
                error_message: "Finnhub API key not configured".to_string(),
            }));
        }

        let url = format!(
            "https://finnhub.io/api/v1/quote?symbol={}&token={}",
            symbol, self.api_key
        );

        match self.client.get(&url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    return Ok(Response::new(GetPriceResponse {
                        symbol,
                        price: 0.0,
                        success: false,
                        error_message: format!("HTTP error: {}", resp.status()),
                    }));
                }
                match resp.json::<FinnhubQuote>().await {
                    Ok(quote) => {
                        if quote.c == 0.0 {
                            Ok(Response::new(GetPriceResponse {
                                symbol,
                                price: 0.0,
                                success: false,
                                error_message: "Symbol not found or no data".to_string(),
                            }))
                        } else {
                            Ok(Response::new(GetPriceResponse {
                                symbol,
                                price: quote.c,
                                success: true,
                                error_message: "".to_string(),
                            }))
                        }
                    }
                    Err(e) => Ok(Response::new(GetPriceResponse {
                        symbol,
                        price: 0.0,
                        success: false,
                        error_message: format!("Failed to parse JSON: {}", e),
                    })),
                }
            }
            Err(e) => Ok(Response::new(GetPriceResponse {
                symbol,
                price: 0.0,
                success: false,
                error_message: format!("HTTP request failed: {}", e),
              })),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("FINNHUB_API_KEY").unwrap_or_default();
    let port = std::env::var("PORT").unwrap_or_else(|_| "50052".to_string());
    let addr = format!("0.0.0.0:{}", port).parse()?;

    println!("Market Integration Service running on {}", addr);
    if api_key.is_empty() {
        println!("WARNING: FINNHUB_API_KEY is empty. Real stock queries will fail.");
    }

    let service = MarketIntegration::new(api_key);

    Server::builder()
        .add_service(MarketIntegrationServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
