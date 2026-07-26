FROM debian:bookworm-slim AS observer-builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends gcc libc6-dev libclips-dev libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY ferric-clips-observer.c /src/ferric-clips-observer.c
COPY ferric-clips-launcher.c /src/ferric-clips-launcher.c

RUN gcc -std=c11 -O2 -Wall -Wextra -Werror \
    -o /ferric-clips-observer /src/ferric-clips-observer.c \
    -l:libclips.so.6 -lcrypto -lm \
    && gcc -std=c11 -O2 -Wall -Wextra -Werror \
    -o /ferric-clips-launcher /src/ferric-clips-launcher.c

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends clips ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=observer-builder /ferric-clips-observer /usr/local/bin/ferric-clips-observer
COPY --from=observer-builder /ferric-clips-launcher /usr/local/bin/ferric-clips-launcher

WORKDIR /workspace

ENTRYPOINT ["/usr/local/bin/ferric-clips-launcher"]
