# Mini Exchange Portfolio System

A microservice-based trading portfolio system built in Rust. It utilizes **gRPC** for internal service communication and exposes a **REST API Gateway** for client requests. Real stock price quotes are integrated with the **Finnhub API**, with automatic fallback to simulated mock market data when the integration is disabled or offline.

## System Architecture

```
                      ┌──────────────────────────────────────┐
                      │             API Client               │
                      └──────────────────┬───────────────────┘
                                         │ HTTP (Port 8080)
                                         ▼
                      ┌──────────────────────────────────────┐
                      │             API Gateway              │
                      └──────┬────────────────────────┬──────┘
                             │ gRPC (50051)           │ gRPC (50053)
                             ▼                        ▼
  ┌─────────────────────────────┐        ┌─────────────────────────────┐
  │       Market Service        │        │      Portfolio Service      │
  └──────────────┬──────────────┘        └──────────────┬──────────────┘
                 │ gRPC (50052)                         │ gRPC (50054)
                 ▼                                      ▼
  ┌─────────────────────────────┐        ┌─────────────────────────────┐
  │ Market Integration Service  │        │        Audit Service        │
  │      (Finnhub API)          │        │       (Redis: audit_logs)    │
  └─────────────────────────────┘        └─────────────────────────────┘
                                                        │
                                                        ▼
                                                 (Postgres: exchange)
```

1. **API Gateway (HTTP: `8080`)**: Exposes REST endpoints to clients and forwards requests to the internal services via gRPC.
2. **Market Service (gRPC: `50051`)**: Fetches prices by querying the `Market Integration Service`. If the integration service fails or is not configured, it gracefully falls back to mock prices (with simulated random fluctuations).
3. **Market Integration Service (gRPC: `50052`)**: Directly connects to the third-party **Finnhub API** to retrieve real-time quotes.
4. **Portfolio Service (gRPC: `50053`)**: Manages cash balances, holdings, and order transaction execution within a PostgreSQL transaction block.
5. **Audit Service (gRPC: `50054`)**: Records structured event logs (`ORDER_CREATED`, `ORDER_EXECUTED`, `ORDER_REJECTED`) inside Redis list (named \).

## REST API Endpoints (Exposed by API Gateway)

### Market Endpoints
* **`GET /docs`**: Interactive Swagger UI API documentation.
* **`GET /openapi.json`**: OpenAPI 3.0 specification file.
* **`GET /symbols`**: Get list of tradeable stock symbols.
* **`GET /prices`**: Get current prices of all symbols.
* **`GET /prices/:symbol`**: Get price of a specific symbol.

### Portfolio Endpoints
* **`GET /portfolio/:userId`**: Get user's balance and current stock holdings. (New user accounts are auto-seeded with a default balance of `$10,000.0` for convenience).
* **`POST /orders`**: Submit a market order.
  * **Payload Structure**:
    ```json
    {
      "userId": "user123",
      "type": "BUY", // or "SELL"
      "symbol": "AAPL",
      "quantity": 10
    }
    ```
* **`GET /orders/:orderId`**: Retrieve transaction details for a specific order.

---

## Getting Started

### Prerequisites
* [Docker](https://www.docker.com/) and [Docker Compose](https://docs.docker.com/compose/)
* Alternatively, [Rust](https://www.rust-lang.org/) (toolchain 1.80+) and `protoc` installed locally.

### Step 1: Configure Environment
Create a `.env` file at the project root:
```ini
FINNHUB_API_KEY=your_finnhub_api_key_here
```
*(If left empty or omitted, the system will fall back to using mocked prices automatically).*

### Step 2: Run with Docker Compose
To build and start all microservices together:
```bash
docker-compose up --build
```
This launches:
* **API Gateway** on port `8080` (HTTP)
* **Market Service** on port `50051` (gRPC)
* **Market Integration Service** on port `50052` (gRPC)
* **Portfolio Service** on port `50053` (gRPC)
* **Audit Service** on port `50054` (gRPC)

### Step 3: Run Tests
You can run the full suite of integration tests locally:
```bash
cargo test
```
The integration tests cover:
* Successful stock purchases (`BUY` with sufficient funds)
* Successful stock sales (`SELL` with sufficient assets)
* Order rejection scenarios (insufficient balance or assets)
* Fallback mechanism verification (graceful handling of Market Service offline errors)

---

## Technical Details

* **Protobuf Definitions**: Located inside [proto/proto/exchange.proto](proto/proto/exchange.proto).
* **Databases**: Redis list (named \)s (`portfolio.db` and `audit.db`) are created automatically in the container/workspace on service launch.
