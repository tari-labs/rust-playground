FROM shepmaster/rust-nightly:sources

RUN rustup component add clippy

RUN cargo clippy
RUN rm src/*.rs

ENTRYPOINT ["/playground/tools/entrypoint.sh"]
