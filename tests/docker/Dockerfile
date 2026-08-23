# Clean-machine end-to-end test: build pxh, then install it onto a stock
# Debian image with system bash and zsh and walk the documented user journey
# (install -> interactive shell -> record -> search -> sync) in e2e_test.sh.

FROM rust:slim-bookworm AS build
WORKDIR /build
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    zsh \
    bsdutils \
    sqlite3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /build/target/release/pxh /usr/local/bin/pxh
COPY tests/docker/e2e_test.sh /e2e_test.sh
RUN chmod +x /e2e_test.sh

CMD ["/e2e_test.sh"]
