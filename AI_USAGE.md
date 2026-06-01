# AI Agent Development Log (AI_USAGE.md)

This log documents the usage of the AI coding agent (Antigravity) in designing, building, and verifying the Mini Exchange Portfolio System.

## Tools Used
* **AI Agent**: Antigravity (Advanced Agentic Coding, Google DeepMind)
* **Underlying Runtime**: Codex CLI
* **Validation utilities**: Cargo toolchain, Rust compiler (`rustc`), Docker Compose, gRPC compilation.

## Key Prompts
* **Prompt 1 (Initial Setup)**: "Lên plan rõ ràng và goal để hoàn thành toàn bộ các yêu cầu trên; tích hợp với data thật (Finnhub API); đặt tên service kết nối nguồn giá thứ ba; làm API gateway sử dụng gRPC để liên lạc giữa các service."
* **Prompt 2 (Test Fixes)**: "Fix compile issues on integration tests related to tokio TcpListener stream conversions and gRPC struct namespaces."

## Tasks Delegated to AI
* Scaffolding the workspace directory structure and configuring package dependencies (`Cargo.toml`).
* Generating gRPC Protocol Buffer contracts (`proto/proto/exchange.proto`).
* Designing and implementing microservice logic: API Gateway (HTTP translation), Market Integration Service (Finnhub), Market Service (with fallback), Portfolio Service (PostgreSQL transactions), and Audit Service (event logs).
* Scaffolding Dockerfiles for all microservices and orchestrating them via `docker-compose.yml`.
* Setting up programmatic integration tests in Rust without network binding conflicts.

## Accepted vs. Modified Code
* **Accepted**:
  * The Axum routing setup in the API Gateway.
  * The database transaction logic inside `portfolio-service`.
  * Multi-stage build structure in the Dockerfiles.
* **Modified**:
  * Splitted `portfolio-service` into library and binary formats (`lib.rs` and `main.rs`) to ensure that integration tests could easily import the struct logic without compiler issues.
  * Extracted the duplicate protobuf generation from the tests to prevent struct mismatch errors.

## Example of Incorrect AI Output & Resolution
* **Problem**: When generating `portfolio-service/tests/integration_tests.rs`, the AI attempted to convert standard `std::net::TcpListener` directly to `tokio::net::TcpListener` inside a tokio task without marking it nonblocking first:
  ```rust
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  // ... tokio::net::TcpListener::from_std(listener) -> Panics at runtime!
  ```
  This caused a runtime panic: `"Registering a blocking socket with the tokio runtime is unsupported"`.
* **Resolution**: Corrected the logic by explicitly setting the socket to non-blocking prior to instantiation:
  ```rust
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  listener.set_nonblocking(true).unwrap(); // Fixed panic
  ```

## Postgres and Redis Refactor
* **Modification**: Migrated databases to PostgreSQL (`portfolio-service`) and Redis (`audit-service`).
* **Implementation Details**:
  - `portfolio-service` queries were written using PostgreSQL syntax (`?`) to Postgres query parameter syntax (`$1`, `$2`, etc.) and transactions were updated to run on `PgPool`.
  - `audit-service` was refactored to write events into a Redis List named `audit_logs` using asynchronous Redis `LPUSH` commands.
  - The root `docker-compose.yml` was updated to deploy these services together with Postgres and Redis infrastructure.

## Swagger API Documentation
* **Implementation Details**:
  - Embedded Swagger UI and OpenAPI 3.0 specification into `api-gateway`.
  - Added endpoints `/docs` (HTML documentation UI) and `/openapi.json` (OpenAPI specification) mapping all REST Gateway handlers.
