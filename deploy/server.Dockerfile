# Amanfold sync server image. Build from the repository root:
#   docker build -f deploy/server.Dockerfile -t amanfold-server .
FROM rust:1.91-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build -p amanfold-server --release --locked \
 && install -D -m 0755 target/release/amanfold-server /out/amanfold-server \
 && mkdir -p /out/data

# Distroless: glibc and nothing else (no shell, no package manager).
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/amanfold-server /usr/local/bin/amanfold-server
COPY --from=build --chown=65532:65532 /out/data /data
USER 65532:65532
ENV AMANFOLD_LISTEN=0.0.0.0:8080 \
    AMANFOLD_DB=/data/amanfold.db \
    RUST_LOG=info
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/amanfold-server"]
CMD ["serve"]
