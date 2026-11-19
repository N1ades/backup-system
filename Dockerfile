# Built by GitHub Actions (.github/workflows/release.yml) and published to
# ghcr.io/n1ades/backup-system. The server only pulls the small runtime image.
FROM rust:1.85-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked && strip target/release/backup-system

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/backup-system /usr/local/bin/backup-system
# DOWNLOAD_DIR=./downloads in .env resolves against this directory.
WORKDIR /app
ENTRYPOINT ["backup-system"]
CMD ["--help"]
