use tonic::{transport::Server, Request, Response, Status};
use std::collections::HashMap;
use std::sync::Mutex;
use rand::Rng;

pub mod exchange {
    tonic::include_proto!("exchange");
}

use exchange::market_service_server::{MarketService, MarketServiceServer};
use exchange::market_integration_service_client::MarketIntegrationServiceClient;
use exchange::{GetPriceRequest, GetPriceResponse, GetPricesRequest, GetPricesResponse, GetSymbolsRequest, GetSymbolsResponse};

pub struct Market {
    integration_url: String,
    static_prices: Mutex<HashMap<String, f64>>,
}

impl Market {
    pub fn new(integration_url: String) -> Self {
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_string(), 150.0);
        prices.insert("MSFT".to_string(), 300.0);
        prices.insert("TSLA".to_string(), 250.0);
        prices.insert("AMZN".to_string(), 180.0);
        prices.insert("GOOG".to_string(), 120.0);

        Self {
            integration_url,
            static_prices: Mutex::new(prices),
        }
    }

    fn get_mock_price(&self, symbol: &str) -> f64 {
        let mut prices = self.static_prices.lock().unwrap();
        if let Some(&price) = prices.get(symbol) {
            // Add a small random fluctuation (-1% to +1%) to simulate active market
            let mut rng = rand::thread_rng();
            let fluctuation: f64 = rng.gen_range(-0.01..0.01);
            let new_price = price * (1.0 + fluctuation);
            prices.insert(symbol.to_string(), new_price);
            new_price
        } else {
            // Default random price for unknown symbols
            let mut rng = rand::thread_rng();
            rng.gen_range(50.0..500.0)
        }
    }
}

#[tonic::async_trait]
impl MarketService for Market {
    async fn get_symbols(
        &self,
        _request: Request<GetSymbolsRequest>,
    ) -> Result<Response<GetSymbolsResponse>, Status> {
        let symbols = vec![
            "AAPL".to_string(),
            "MSFT".to_string(),
            "TSLA".to_string(),
            "AMZN".to_string(),
            "GOOG".to_string(),
        ];
        Ok(Response::new(GetSymbolsResponse { symbols }))
    }

    async fn get_price(
        &self,
        request: Request<GetPriceRequest>,
    ) -> Result<Response<GetPriceResponse>, Status> {
        let req = request.into_inner();
        let symbol = req.symbol.to_uppercase();

        // 1. Try to fetch from market-integration-service
        match MarketIntegrationServiceClient::connect(self.integration_url.clone()).await {
            Ok(mut client) => {
                let grpc_req = tonic::Request::new(GetPriceRequest {
                    symbol: symbol.clone(),
                });
                match client.get_finnhub_price(grpc_req).await {
                    Ok(grpc_resp) => {
                        let resp_data = grpc_resp.into_inner();
                        if resp_data.success {
                            return Ok(Response::new(GetPriceResponse {
                                symbol: symbol.clone(),
                                price: resp_data.price,
                                success: true,
                                error_message: "".to_string(),
                            }));
                        } else {
                            println!(
                                "Market Integration failed for symbol {}: {}. Falling back to mock price.",
                                symbol, resp_data.error_message
                            );
                        }
                    }
                    Err(e) => {
                        println!(
                            "Market Integration gRPC call failed for symbol {}: {}. Falling back to mock price.",
                            symbol, e
                        );
                    }
                }
            }
            Err(e) => {
                println!(
                    "Failed to connect to Market Integration Service (url: {}): {}. Falling back to mock price.",
                    self.integration_url, e
                );
            }
        }

        // 2. Fallback to mock price
        let mock_price = self.get_mock_price(&symbol);
        Ok(Response::new(GetPriceResponse {
            symbol: symbol.clone(),
            price: mock_price,
            success: true,
            error_message: "".to_string(),
        }))
    }

    async fn get_prices(
        &self,
        _request: Request<GetPricesRequest>,
    ) -> Result<Response<GetPricesResponse>, Status> {
        let symbols = vec![
            "AAPL".to_string(),
            "MSFT".to_string(),
            "TSLA".to_string(),
            "AMZN".to_string(),
            "GOOG".to_string(),
        ];
        let mut prices = HashMap::new();

        for symbol in symbols {
            // Attempt to get price (either from integration or mock)
            let req = Request::new(GetPriceRequest { symbol: symbol.clone() });
            if let Ok(resp) = self.get_price(req).await {
                prices.insert(symbol, resp.into_inner().price);
            } else {
                prices.insert(symbol.clone(), self.get_mock_price(&symbol));
            }
        }

        Ok(Response::new(GetPricesResponse { prices }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let port = std::env::var("PORT").unwrap_or_else(|_| "50051".to_string());
    let integration_url = std::env::var("MARKET_INTEGRATION_URL")
        .unwrap_or_else(|_| "http://localhost:50052".to_string());
    let addr = format!("0.0.0.0:{}", port).parse()?;

    println!("Market Service running on {}", addr);
    println!("Connecting to Market Integration Service at {}", integration_url);

    let service = Market::new(integration_url);

    Server::builder()
        .add_service(MarketServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
