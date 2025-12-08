FROM rust:1.75
WORKDIR /app
COPY . .
RUN cargo build -p indexer-core --bin indexer --release
CMD ["./target/release/indexer"]
