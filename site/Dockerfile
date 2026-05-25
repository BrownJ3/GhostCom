FROM rust:1.95-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN useradd --create-home --shell /usr/sbin/nologin appuser
WORKDIR /app
COPY --from=builder /app/target/release/ghostcom-site /usr/local/bin/ghostcom-site
USER appuser
ENV PORT=8080
EXPOSE 8080
CMD ["ghostcom-site"]

