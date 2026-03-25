# Oracle test image — pre-compiled test binary, no Rust toolchain needed at runtime
#
# Build: docker build -f eval/k8s/Dockerfile -t false-systems/rauta-oracle .
# Run:   docker run --rm -e RAUTA_PROXY_ENDPOINT=http://host:8080 false-systems/rauta-oracle

FROM rust:1.83-slim AS builder

WORKDIR /build
COPY eval/oracle/ eval/oracle/

RUN cargo test --manifest-path eval/oracle/Cargo.toml --no-run --release 2>&1 \
    && cp $(find eval/oracle/target/release/deps -name 'rauta_oracle-*' -type f ! -name '*.d' | head -1) /oracle

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /oracle /usr/local/bin/oracle

ENTRYPOINT ["/usr/local/bin/oracle", "--nocapture"]
