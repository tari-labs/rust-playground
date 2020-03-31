FROM quay.io/tarilabs/rust_tari-build-with-deps:nightly-2020-01-08
ARG DOCKER=docker-19.03.8
USER root
ADD https://download.docker.com/linux/static/stable/x86_64/$DOCKER.tgz /
RUN cd /usr/bin/ && tar --strip-components=1 -xzf /$DOCKER.tgz docker/docker

COPY Cargo.lock Cargo.toml /app/
COPY src /app/src/

WORKDIR /app

RUN cargo build --release
RUN cargo install --path .

COPY frontend/build /frontend
ENV TMPDIR=/playground \
    RUST_LOG=info \
    PLAYGROUND_UI_ADDRESS=127.0.0.1 \
    PLAYGROUND_UI_PORT=5000 \
    PLAYGROUND_UI_ROOT=/frontend \
    PLAYGROUND_GITHUB_TOKEN=deadbeef \
    PLAYGROUND_CORS_ENABLED=1

ENTRYPOINT []
# Allow ctrl+C to work by wrapping the playground in bash
# (it will also perform process reaping). The odd construction
# is because if bash recognises a simple command it'll just
# exec it directly, so add a useless additional command.
CMD ["bash", "-c", "ui || false"]