FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends clips ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

ENTRYPOINT ["clips"]
