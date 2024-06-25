# syntax=docker/dockerfile:1.6
ARG RUST_VERSION=1.79
ARG CARGO_BUILD_FEATURES=default
ARG RUST_RELEASE_MODE=debug

ARG AMD_BUILDER_IMAGE=rust:${RUST_VERSION}

ARG AMD_RUNNER_IMAGE=debian:bookworm-slim

ARG UNAME=cleanup
ARG UID=1000
ARG GID=1000

# AMD64 builder
FROM --platform=${BUILDPLATFORM} ${AMD_BUILDER_IMAGE} AS build-amd64

ARG CARGO_BUILD_FEATURES
ARG RUST_RELEASE_MODE
ARG RUSTFLAGS

WORKDIR /lemmy

COPY . ./

# Debug build
RUN --mount=type=cache,target=/lemmy-thumbnail-cleaner/target set -ex; \
    if [ "${RUST_RELEASE_MODE}" = "debug" ]; then \
        cargo build --features "${CARGO_BUILD_FEATURES}"; \
        mv target/"${RUST_RELEASE_MODE}"/lemmy-thumbnail-cleaner ./lemmy-thumbnail-cleaner; \
    fi

# Release build
RUN --mount=type=cache,target=/lemmy-thumbnail-cleaner/target set -ex; \
    if [ "${RUST_RELEASE_MODE}" = "release" ]; then \
        [ -z "$USE_RELEASE_CACHE" ] && cargo clean --release; \
        cargo build --features "${CARGO_BUILD_FEATURES}" --release; \
        mv target/"${RUST_RELEASE_MODE}"/lemmy-thumbnail-cleaner ./lemmy-thumbnail-cleaner; \
    fi

# amd64 base runner
FROM ${AMD_RUNNER_IMAGE} AS runner-linux-amd64

# libpq-dev is required for the postgresql client, others are kept for the possible ssl support
RUN apt update && apt install -y libpq-dev ca-certificates libssl-dev

COPY --from=build-amd64 --chmod=0755 /lemmy/lemmy-thumbnail-cleaner /usr/local/bin


# Final image that use a base runner based on the target OS and ARCH
FROM runner-${TARGETOS}-${TARGETARCH}

LABEL org.opencontainers.image.source="https://github.com/wereii/lemmy-thumbnail-cleaner"
LABEL org.opencontainers.image.licenses="MIT"

ARG UNAME
ARG GID
ARG UID

RUN groupadd -g ${GID} -o ${UNAME} && \
    useradd -m -u ${UID} -g ${GID} -o -s /bin/bash ${UNAME}
USER $UNAME

ENTRYPOINT ["lemmy-thumbnail-cleaner"]