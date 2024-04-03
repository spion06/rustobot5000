# Stage 1: Build the binary with musl
FROM rust:latest as builder

# Add musl target
RUN rustup target add x86_64-unknown-linux-gnu

# Create a new empty shell project
WORKDIR /usr/src/rustobot5000
COPY . .

RUN apt-get update && apt-get install musl-tools -y && apt-get -y install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
      gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
      gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
      gstreamer1.0-libav libgstrtspserver-1.0-dev libges-1.0-dev && \
      apt-get -y install libssl-dev
# Build the binary for musl target
RUN cargo build --release --target x86_64-unknown-linux-gnu

# Stage 2: Create the final image from scratch
FROM debian:stable-slim
RUN apt-get update && apt-get upgrade -y && apt-get -y --no-install-recommends install gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav libgstrtspserver-1.0-dev libges-1.0-dev && \
    apt-get clean autoclean && \
    apt-get autoremove --yes && \
    rm -rf /var/lib/{apt,dpkg,cache,log}/

# Copy the statically-linked binary from the builder stage
COPY --from=builder /usr/src/rustobot5000/target/x86_64-unknown-linux-gnu/release/rustobot5000 /rustobot5000

# Command to run when starting the container
CMD ["/rustobot5000"]
