FROM rust:1.75 AS builder
WORKDIR /app
COPY . .
RUN cd ops/faucet && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/faucet /usr/local/bin/faucet
ENV RPC_URL=http://validator1:8545
ENV CHAIN_ID=kova-testnet
ENV FAUCET_AMOUNT=100000
EXPOSE 8080
CMD ["faucet"]
