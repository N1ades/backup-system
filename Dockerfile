FROM rust:1.85-bookworm

RUN cargo install cargo-watch --locked

WORKDIR /app
COPY . .

RUN cargo fetch

CMD ["cargo", "watch", "-x", "run -- run"]
