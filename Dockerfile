# Stage 1: Build all binaries in the workspace
FROM rust:1.88-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release

# Stage 2: api-gateway
FROM debian:bookworm-slim AS api-gateway
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/api-gateway /app/api-gateway
EXPOSE 8080
CMD ["/app/api-gateway"]

# Stage 3: market-service
FROM debian:bookworm-slim AS market-service
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/market-service /app/market-service
EXPOSE 50051
CMD ["/app/market-service"]

# Stage 4: market-integration-service
FROM debian:bookworm-slim AS market-integration-service
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/market-integration-service /app/market-integration-service
EXPOSE 50052
CMD ["/app/market-integration-service"]

# Stage 5: portfolio-service
FROM debian:bookworm-slim AS portfolio-service
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/portfolio-service /app/portfolio-service
EXPOSE 50053
CMD ["/app/portfolio-service"]

# Stage 6: audit-service
FROM debian:bookworm-slim AS audit-service
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/audit-service /app/audit-service
EXPOSE 50054
CMD ["/app/audit-service"]
