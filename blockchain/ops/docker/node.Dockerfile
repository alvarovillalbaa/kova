FROM rust:1.75
WORKDIR /app
COPY . .
RUN cargo build -p node --release
CMD ["./target/release/node"]
