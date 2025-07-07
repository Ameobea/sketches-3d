FROM debian:12.11-slim AS builder

RUN apt-get update && apt-get install -y curl build-essential pkg-config libssl-dev

# Install rust
RUN curl https://sh.rustup.rs/ -sSf | \
  sh -s -- -y --default-toolchain nightly-2025-07-04

ENV PATH="/root/.cargo/bin:${PATH}"

ADD . ./

RUN RUSTFLAGS="--cfg tokio_unstable --cfg foundations_unstable" cargo build --release

FROM debian:12.11-slim

RUN apt-get update && apt-get install -y libssl-dev ca-certificates && update-ca-certificates

COPY --from=builder \
  /target/release/geoscript_backend \
  /usr/local/bin/
WORKDIR /root
CMD /usr/local/bin/geoscript_backend
