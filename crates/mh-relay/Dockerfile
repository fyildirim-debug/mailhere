# Mailhere bildirim aktarıcısı. Bağlam deponun köküdür (çalışma alanı ve
# Cargo.lock gerekir): docker build -f crates/mh-relay/Dockerfile .
FROM rust:1-bookworm AS derleme
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p mh-relay

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data relay \
 && mkdir -p /data && chown relay /data
COPY --from=derleme /src/target/release/mailhere-relay /usr/local/bin/mailhere-relay
USER relay
ENV MH_RELAY_DATA=/data PORT=8080
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=60s --timeout=5s CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/8080"]
CMD ["mailhere-relay"]
