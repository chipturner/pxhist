FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    zsh \
    sqlite3 \
    curl \
    ca-certificates \
    build-essential \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY . .

RUN cargo build --release
ENV PATH="/build/target/release:${PATH}"

COPY tests/docker/e2e_test.sh /e2e_test.sh
RUN chmod +x /e2e_test.sh

CMD ["/e2e_test.sh"]
