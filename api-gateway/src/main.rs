use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

pub mod exchange {
    tonic::include_proto!("exchange");
}

use exchange::market_service_client::MarketServiceClient;
use exchange::portfolio_service_client::PortfolioServiceClient;
use exchange::{GetOrderRequest, GetPortfolioRequest, GetPriceRequest, GetPricesRequest, GetSymbolsRequest, CreateOrderRequest};

struct AppState {
    market_url: String,
    portfolio_url: String,
}

#[derive(Deserialize, Debug)]
struct CreateOrderPayload {
    #[serde(alias = "userId", alias = "user_id")]
    user_id: String,
    #[serde(alias = "type", alias = "order_type", alias = "orderType")]
    order_type: String,
    symbol: String,
    quantity: f64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let market_url = std::env::var("MARKET_URL").unwrap_or_else(|_| "http://localhost:50051".to_string());
    let portfolio_url = std::env::var("PORTFOLIO_URL").unwrap_or_else(|_| "http://localhost:50053".to_string());

    let state = Arc::new(AppState {
        market_url,
        portfolio_url,
    });

    let app = Router::new()
        .route("/symbols", get(get_symbols))
        .route("/prices", get(get_prices))
        .route("/prices/:symbol", get(get_price_by_symbol))
        .route("/portfolio/:userId", get(get_portfolio))
        .route("/orders", post(create_order))
        .route("/orders/:orderId", get(get_order_by_id))
        .route("/openapi.json", get(get_openapi_json))
        .route("/docs", get(get_swagger_ui))
        .route("/", get(get_swagger_ui)) // Redirect root to Swagger
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("API Gateway running on HTTP port {}", port);
    println!("Swagger UI available at http://localhost:{}/docs", port);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn get_swagger_ui() -> impl IntoResponse {
    Html(r#"
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>Mini Exchange Portfolio API Docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
  <style>
    html { box-sizing: border-box; overflow-y: scroll; }
    *, *:before, *:after { box-sizing: inherit; }
    body { margin: 0; background: #fafafa; }
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-standalone-preset.js"></script>
  <script>
    window.onload = () => {
      window.ui = SwaggerUIBundle({
        url: '/openapi.json',
        dom_id: '#swagger-ui',
        deepLinking: true,
        presets: [
          SwaggerUIBundle.presets.apis,
          SwaggerUIStandalonePreset
        ],
        layout: "BaseLayout"
      });
    };
  </script>
</body>
</html>
"#)
}

async fn get_openapi_json() -> impl IntoResponse {
    let openapi_spec = r#"{
  "openapi": "3.0.0",
  "info": {
    "title": "Mini Exchange Portfolio API",
    "version": "1.0.0",
    "description": "REST API Gateway translating client calls to internal gRPC microservices."
  },
  "paths": {
    "/symbols": {
      "get": {
        "summary": "Get tradeable stock symbols",
        "responses": {
          "200": {
            "description": "List of symbols",
            "content": {
              "application/json": {
                "schema": {
                  "type": "array",
                  "items": { "type": "string" }
                }
              }
            }
          }
        }
      }
    },
    "/prices": {
      "get": {
        "summary": "Get prices of all symbols",
        "responses": {
          "200": {
            "description": "Symbol to price mapping",
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "additionalProperties": { "type": "number" }
                }
              }
            }
          }
        }
      }
    },
    "/prices/{symbol}": {
      "get": {
        "summary": "Get price of a specific symbol",
        "parameters": [
          {
            "name": "symbol",
            "in": "path",
            "required": true,
            "schema": { "type": "string" }
          }
        ],
        "responses": {
          "200": {
            "description": "Real or mock price details",
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "symbol": { "type": "string" },
                    "price": { "type": "number" }
                  }
                }
              }
            }
          },
          "404": { "description": "Symbol not found" }
        }
      }
    },
    "/portfolio/{userId}": {
      "get": {
        "summary": "Get user balance and stock holdings",
        "parameters": [
          {
            "name": "userId",
            "in": "path",
            "required": true,
            "schema": { "type": "string" }
          }
        ],
        "responses": {
          "200": {
            "description": "User portfolio",
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "userId": { "type": "string" },
                    "balance": { "type": "number" },
                    "holdings": {
                      "type": "object",
                      "additionalProperties": { "type": "number" }
                    }
                  }
                }
              }
            }
          }
        }
      }
    },
    "/orders": {
      "post": {
        "summary": "Submit buy or sell order",
        "requestBody": {
          "required": true,
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "required": ["userId", "type", "symbol", "quantity"],
                "properties": {
                  "userId": { "type": "string" },
                  "type": { "type": "string", "enum": ["BUY", "SELL"] },
                  "symbol": { "type": "string" },
                  "quantity": { "type": "number" }
                }
              }
            }
          }
        },
        "responses": {
          "201": {
            "description": "Order executed successfully",
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "orderId": { "type": "string" },
                    "userId": { "type": "string" },
                    "orderType": { "type": "string" },
                    "symbol": { "type": "string" },
                    "quantity": { "type": "number" },
                    "price": { "type": "number" },
                    "status": { "type": "string" },
                    "reason": { "type": "string" },
                    "createdAt": { "type": "string" }
                  }
                }
              }
            }
          },
          "400": { "description": "Order rejected (insufficient cash or stock holdings)" }
        }
      }
    },
    "/orders/{orderId}": {
      "get": {
        "summary": "Get details of specific order",
        "parameters": [
          {
            "name": "orderId",
            "in": "path",
            "required": true,
            "schema": { "type": "string" }
          }
        ],
        "responses": {
          "200": {
            "description": "Order history entry details",
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "orderId": { "type": "string" },
                    "userId": { "type": "string" },
                    "orderType": { "type": "string" },
                    "symbol": { "type": "string" },
                    "quantity": { "type": "number" },
                    "price": { "type": "number" },
                    "status": { "type": "string" },
                    "reason": { "type": "string" },
                    "createdAt": { "type": "string" }
                  }
                }
              }
            }
          },
          "404": { "description": "Order not found" }
        }
      }
    }
  }
}"#;

    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        openapi_spec,
    )
}

async fn get_symbols(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut client = match MarketServiceClient::connect(state.market_url.clone()).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    };

    match client.get_symbols(tonic::Request::new(GetSymbolsRequest {})).await {
        Ok(resp) => (StatusCode::OK, Json(resp.into_inner().symbols)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

async fn get_prices(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut client = match MarketServiceClient::connect(state.market_url.clone()).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    };

    match client.get_prices(tonic::Request::new(GetPricesRequest {})).await {
        Ok(resp) => (StatusCode::OK, Json(resp.into_inner().prices)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

async fn get_price_by_symbol(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    let mut client = match MarketServiceClient::connect(state.market_url.clone()).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    };

    match client.get_price(tonic::Request::new(GetPriceRequest { symbol })).await {
        Ok(resp) => {
            let data = resp.into_inner();
            if data.success {
                (StatusCode::OK, Json(serde_json::json!({
                    "symbol": data.symbol,
                    "price": data.price
                }))).into_response()
            } else {
                (StatusCode::NOT_FOUND, Json(ErrorResponse { error: data.error_message })).into_response()
            }
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

async fn get_portfolio(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    let mut client = match PortfolioServiceClient::connect(state.portfolio_url.clone()).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    };

    match client.get_portfolio(tonic::Request::new(GetPortfolioRequest { user_id })).await {
        Ok(resp) => {
            let data = resp.into_inner();
            (StatusCode::OK, Json(serde_json::json!({
                "userId": data.user_id,
                "balance": data.balance,
                "holdings": data.holdings
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

async fn create_order(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateOrderPayload>,
) -> impl IntoResponse {
    let mut client = match PortfolioServiceClient::connect(state.portfolio_url.clone()).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    };

    let req = CreateOrderRequest {
        user_id: payload.user_id,
        order_type: payload.order_type,
        symbol: payload.symbol,
        quantity: payload.quantity,
    };

    match client.create_order(tonic::Request::new(req)).await {
        Ok(resp) => {
            let data = resp.into_inner();
            let status_code = if data.status == "REJECTED" {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::CREATED
            };
            (status_code, Json(serde_json::json!({
                "orderId": data.order_id,
                "userId": data.user_id,
                "orderType": data.order_type,
                "symbol": data.symbol,
                "quantity": data.quantity,
                "price": data.price,
                "status": data.status,
                "reason": data.reason,
                "createdAt": data.created_at
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

async fn get_order_by_id(
    State(state): State<Arc<AppState>>,
    Path(order_id): Path<String>,
) -> impl IntoResponse {
    let mut client = match PortfolioServiceClient::connect(state.portfolio_url.clone()).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    };

    match client.get_order(tonic::Request::new(GetOrderRequest { order_id })).await {
        Ok(resp) => {
            let data = resp.into_inner();
            (StatusCode::OK, Json(serde_json::json!({
                "orderId": data.order_id,
                "userId": data.user_id,
                "orderType": data.order_type,
                "symbol": data.symbol,
                "quantity": data.quantity,
                "price": data.price,
                "status": data.status,
                "reason": data.reason,
                "createdAt": data.created_at
            }))).into_response()
        }
        Err(e) => {
            let status = match e.code() {
                tonic::Code::NotFound => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(ErrorResponse { error: e.message().to_string() })).into_response()
        }
    }
}
