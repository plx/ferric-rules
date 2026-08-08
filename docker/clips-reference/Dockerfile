ARG DEBIAN_BASE_IMAGE=debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241
ARG CLIPS_DEBIAN_VERSION=6.30-4.1

FROM ${DEBIAN_BASE_IMAGE} AS observer-builder

ARG CLIPS_DEBIAN_VERSION

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        gcc \
        libc6-dev \
        libclips-dev="${CLIPS_DEBIAN_VERSION}" \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY ferric-clips-observer.c /src/ferric-clips-observer.c
COPY ferric-clips-launcher.c /src/ferric-clips-launcher.c

RUN gcc -std=c11 -O2 -Wall -Wextra -Werror \
    -o /ferric-clips-observer /src/ferric-clips-observer.c \
    -l:libclips.so.6 -lcrypto -lm \
    && gcc -std=c11 -O2 -Wall -Wextra -Werror \
    -o /ferric-clips-launcher /src/ferric-clips-launcher.c

FROM ${DEBIAN_BASE_IMAGE}

ARG DEBIAN_BASE_IMAGE
ARG CLIPS_DEBIAN_VERSION

LABEL org.opencontainers.image.base.name="${DEBIAN_BASE_IMAGE}" \
      org.ferric-rules.clips.engine-version="6.30" \
      org.ferric-rules.clips.package-version="${CLIPS_DEBIAN_VERSION}"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        clips="${CLIPS_DEBIAN_VERSION}" \
        libclips="${CLIPS_DEBIAN_VERSION}" \
    && rm -rf /var/lib/apt/lists/*

COPY --from=observer-builder /ferric-clips-observer /usr/local/bin/ferric-clips-observer
COPY --from=observer-builder /ferric-clips-launcher /usr/local/bin/ferric-clips-launcher

RUN set -eu; \
    package_version="$(dpkg-query -W -f='${Version}' clips)"; \
    test "$package_version" = "$CLIPS_DEBIAN_VERSION"; \
    architecture="$(dpkg --print-architecture)"; \
    case "$architecture" in \
      amd64) platform="linux/amd64" ;; \
      arm64) platform="linux/arm64" ;; \
      *) echo "unsupported CLIPS reference architecture: $architecture" >&2; exit 1 ;; \
    esac; \
    library_path="$(find /usr/lib -type f -name 'libclips.so.6.30' -print -quit)"; \
    test -n "$library_path"; \
    binary_sha256="$(sha256sum /usr/bin/clips | cut -d ' ' -f 1)"; \
    library_sha256="$(sha256sum "$library_path" | cut -d ' ' -f 1)"; \
    install -d -m 0755 /usr/local/share/ferric-clips-reference; \
    printf 'FERRIC-CLIPS-PROVENANCE|1|6.30|%s|%s|%s|%s|%s\n' \
      "$package_version" "$platform" "$binary_sha256" "$library_sha256" \
      "$DEBIAN_BASE_IMAGE" \
      > /usr/local/share/ferric-clips-reference/provenance; \
    chmod 0444 /usr/local/share/ferric-clips-reference/provenance

WORKDIR /workspace

ENTRYPOINT ["/usr/local/bin/ferric-clips-launcher"]
