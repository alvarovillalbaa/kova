FROM rust:1.75
WORKDIR /app
COPY . .
RUN cargo build -p sequencer-api --release
CMD ["./target/release/sequencer-api"]

