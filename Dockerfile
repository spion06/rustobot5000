# Stage 1: Build the binary
FROM rust:1.98-trixie AS builder

WORKDIR /usr/src/rustobot5000

# Install build deps BEFORE copying source — this layer is cached across source changes
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    libgstrtspserver-1.0-dev \
    libges-1.0-dev \
    libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY . .

# Cache mounts keep the cargo registry and compiled artifacts between builds.
# The binary is copied out before the RUN ends since cache mounts don't persist in the layer.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/src/rustobot5000/target \
    cargo build --release --locked --target x86_64-unknown-linux-gnu && \
    cp target/x86_64-unknown-linux-gnu/release/rustobot5000 /rustobot5000-bin

# Stage 2: Runtime image
FROM debian:trixie-slim

RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends \
    ca-certificates \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    libgstrtspserver-1.0-0 \
    libges-1.0-0 && \
    apt-get clean autoclean && \
    apt-get autoremove --yes && \
    rm -rf /var/lib/{apt,dpkg,cache,log}/

COPY --from=builder /rustobot5000-bin /rustobot5000

CMD ["/rustobot5000"]
