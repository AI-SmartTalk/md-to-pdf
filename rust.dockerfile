FROM rustlang/rust:nightly-bookworm-slim

RUN rustup component add rustfmt clippy
