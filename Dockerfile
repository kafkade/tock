# syntax=docker/dockerfile:1

# ── Builder ───────────────────────────────────────────────────────────
FROM rust:1.88-bookworm AS builder

WORKDIR /build
COPY . .

RUN cargo build --release -p tock-server \
    && strip target/release/tock-server

# ── Runtime ───────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for the server process.
RUN useradd --system --create-home --shell /usr/sbin/nologin tock

COPY --from=builder /build/target/release/tock-server /usr/local/bin/tock-server
COPY --from=builder /build/crates/tock-server/LICENSE /usr/share/doc/tock-server/LICENSE

# Persistent data volume.
RUN mkdir -p /var/lib/tock-server && chown tock:tock /var/lib/tock-server
VOLUME /var/lib/tock-server

USER tock

ENV TOCK_BIND=0.0.0.0:8080
ENV TOCK_DATA_DIR=/var/lib/tock-server
ENV RUST_LOG=info

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD ["tock-server", "--help"]

ENTRYPOINT ["tock-server"]
