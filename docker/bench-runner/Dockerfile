FROM rust:1.93-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends clips jq bc wget \
    && rm -rf /var/lib/apt/lists/*

# Install hyperfine from GitHub release (arch-aware, matches CI version)
RUN ARCH="$(dpkg --print-architecture)" \
    && wget -q "https://github.com/sharkdp/hyperfine/releases/download/v1.18.0/hyperfine_1.18.0_${ARCH}.deb" \
    && dpkg -i "hyperfine_1.18.0_${ARCH}.deb" \
    && rm "hyperfine_1.18.0_${ARCH}.deb"

WORKDIR /workspace
