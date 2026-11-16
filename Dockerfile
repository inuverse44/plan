# Builds everything from source: types → WASM → UI → CLI. The result is a single static-ish binary that serves the UI and the API.
FROM rust:1.91-bookworm AS build
ARG WASM_BINDGEN=0.2.100
RUN rustup target add wasm32-unknown-unknown \
 && curl -sSfL "https://github.com/rustwasm/wasm-bindgen/releases/download/${WASM_BINDGEN}/wasm-bindgen-${WASM_BINDGEN}-x86_64-unknown-linux-musl.tar.gz" \
    | tar xz -C /usr/local/bin --strip-components=1 "wasm-bindgen-${WASM_BINDGEN}-x86_64-unknown-linux-musl/wasm-bindgen" \
 && apt-get update && apt-get install -y --no-install-recommends nodejs npm && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY web/package.json web/package-lock.json web/
RUN cd web && npm ci --silent
COPY . .
RUN ./build.sh

FROM gcr.io/distroless/cc-debian12
COPY --from=build /src/target/release/plan /plan
# PLAN_STORE=firestore:PROJECT is set on the Cloud Run service
ENTRYPOINT ["/plan", "serve"]
