FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG NODE_VERSION=22.22.2
ARG RUST_VERSION=1.97.1
ARG TARGETARCH
ARG USER_ID=1000
ARG GROUP_ID=1000

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        file \
        git \
        libayatana-appindicator3-dev \
        libfuse2t64 \
        librsvg2-dev \
        libssl-dev \
        libwebkit2gtk-4.1-dev \
        libxdo-dev \
        openssl \
        patchelf \
        pkg-config \
        wget \
        xdg-utils \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    case "${TARGETARCH:-amd64}" in \
        amd64) node_arch="x64" ;; \
        arm64) node_arch="arm64" ;; \
        *) echo "Unsupported Docker target architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    archive="node-v${NODE_VERSION}-linux-${node_arch}.tar.xz"; \
    base_url="https://nodejs.org/dist/v${NODE_VERSION}"; \
    curl --fail --location --proto '=https' --tlsv1.2 \
        "${base_url}/${archive}" -o "/tmp/${archive}"; \
    curl --fail --location --proto '=https' --tlsv1.2 \
        "${base_url}/SHASUMS256.txt" -o /tmp/SHASUMS256.txt; \
    cd /tmp; \
    grep " ${archive}$" SHASUMS256.txt | sha256sum --check -; \
    mkdir -p /opt/node; \
    tar --extract --xz --strip-components=1 \
        --file "/tmp/${archive}" --directory /opt/node; \
    rm -f "/tmp/${archive}" /tmp/SHASUMS256.txt

RUN groupadd --gid "${GROUP_ID}" builder \
    && useradd \
        --uid "${USER_ID}" \
        --gid "${GROUP_ID}" \
        --create-home \
        --shell /bin/bash \
        builder

ENV HOME=/home/builder
ENV CARGO_HOME=/home/builder/.cargo
ENV RUSTUP_HOME=/home/builder/.rustup
ENV PATH=/home/builder/.cargo/bin:/opt/node/bin:${PATH}
ENV APPIMAGE_EXTRACT_AND_RUN=1

USER builder

RUN set -eux; \
    rustup_installer="$(mktemp)"; \
    curl --fail --proto '=https' --tlsv1.2 \
        https://sh.rustup.rs -o "${rustup_installer}"; \
    sh "${rustup_installer}" -y --profile minimal --default-toolchain "${RUST_VERSION}"; \
    rm -f "${rustup_installer}"; \
    rustup component add --toolchain "${RUST_VERSION}" clippy rustfmt

WORKDIR /workspace

CMD ["bash"]
