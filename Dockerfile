FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        curl \
        ca-certificates \
        perl \
        make \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency compilation by copying manifests first
COPY Cargo.toml Cargo.lock ./
COPY vendor/ vendor/
COPY crates/telemaco-dom/Cargo.toml       crates/telemaco-dom/Cargo.toml
COPY crates/telemaco-net/Cargo.toml       crates/telemaco-net/Cargo.toml
COPY crates/telemaco-browser/Cargo.toml   crates/telemaco-browser/Cargo.toml
COPY crates/telemaco-cdp/Cargo.toml       crates/telemaco-cdp/Cargo.toml
COPY crates/telemaco-js/Cargo.toml        crates/telemaco-js/Cargo.toml
COPY crates/telemaco-mcp/Cargo.toml       crates/telemaco-mcp/Cargo.toml
COPY crates/telemaco-render/Cargo.toml    crates/telemaco-render/Cargo.toml
COPY crates/telemaco-cli/Cargo.toml       crates/telemaco-cli/Cargo.toml
COPY crates/telemaco/Cargo.toml           crates/telemaco/Cargo.toml

# Create stub src files so cargo can resolve the dependency graph
RUN for crate in telemaco-dom telemaco-net telemaco-browser telemaco-cdp telemaco-js telemaco-mcp telemaco-render telemaco; do \
        mkdir -p crates/$crate/src && echo "// stub" > crates/$crate/src/lib.rs; \
    done && \
    mkdir -p crates/telemaco-cli/src && \
    echo "fn main() {}" > crates/telemaco-cli/src/main.rs && \
    echo "fn main() {}" > crates/telemaco-cli/src/worker.rs

RUN cargo build --release --features render --bin telemaco --bin telemaco-worker 2>/dev/null || true

ARG TELEMACO_VERSION

# Copy real sources and build
COPY crates/ crates/
RUN echo "Building Telemaco version ${TELEMACO_VERSION:-from Cargo.toml}" && \
    touch crates/*/src/*.rs && cargo build --release --features render --bin telemaco --bin telemaco-worker

# ---

# distroless/cc: glibc + libgcc + CA certs only — no shell, no package manager
FROM gcr.io/distroless/cc-debian12

COPY --from=builder /build/target/release/telemaco /telemaco
COPY --from=builder /build/target/release/telemaco-worker /telemaco-worker

EXPOSE 9222

# Bind to 0.0.0.0 so the port is reachable via `docker run -p 9222:9222`.
# Native binary still defaults to 127.0.0.1 (loopback only) — this override
# is just for the container.
ENTRYPOINT ["/telemaco"]
CMD ["serve", "--port", "9222", "--host", "0.0.0.0"]
