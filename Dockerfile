FROM oven/bun:1 AS ui
WORKDIR /app/ui
COPY ui/package.json ui/bun.lock ./
RUN bun install --frozen-lockfile
COPY ui/ ./
RUN bun run build

FROM rust:1.98-trixie AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler unzip \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY scripts/fetch-duckdb.sh ./scripts/fetch-duckdb.sh
RUN ./scripts/fetch-duckdb.sh
COPY --from=ui /app/ui/dist ./ui/dist
RUN cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /data otelview
COPY --from=build /app/target/release/otelview /usr/local/bin/otelview
USER otelview
WORKDIR /data
VOLUME /data
EXPOSE 4317 4318 4319
ENTRYPOINT ["otelview"]
CMD ["--listen", "0.0.0.0:4319"]
