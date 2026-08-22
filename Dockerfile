# syntax=docker/dockerfile:1.7
# Production image for the single-process, local-first deployment.

FROM node:22.14.0-bookworm-slim AS ui-builder
WORKDIR /src/ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/ ./
RUN npm run build

FROM rust:1.85.0-bookworm AS rust-builder
WORKDIR /src
RUN rustup toolchain install nightly-2026-07-17 --profile minimal
COPY . .
COPY --from=ui-builder /src/ui/dist ./ui/dist
RUN cargo +nightly-2026-07-17 build --release --bin oaip

FROM debian:12.9-slim AS runtime
RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --home-dir /var/lib/oaip --create-home --shell /usr/sbin/nologin oaip
COPY --from=rust-builder /src/target/release/oaip /usr/local/bin/oaip
COPY --from=rust-builder /src/examples /opt/oaip/examples
COPY --from=rust-builder /src/ui/dist /opt/oaip/ui/dist
COPY --from=rust-builder /src/ontology.odl /opt/oaip/ontology.odl

RUN mkdir -p /var/lib/oaip && chown -R oaip:oaip /var/lib/oaip
USER oaip
WORKDIR /var/lib/oaip

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD wget --quiet --spider http://127.0.0.1:8080/readyz || exit 1

ENTRYPOINT ["/usr/local/bin/oaip"]
CMD ["--db", "/var/lib/oaip/openaip.db", "serve", "--host", "0.0.0.0", "--port", "8080", "--static-dir", "/opt/oaip/ui/dist", "--odl", "/opt/oaip/examples/crm.odl"]
