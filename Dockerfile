FROM rust:1.94-bookworm AS source
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests

FROM source AS test
RUN rustup component add rustfmt clippy
RUN cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked

FROM source AS build
RUN cargo build --locked --release

FROM scratch AS artifact
COPY --from=build /app/target/release/gh-wanted /gh-wanted
