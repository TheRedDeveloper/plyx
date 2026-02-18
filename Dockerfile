# plyx Android build image
# Published to: ghcr.io/thereddeveloper/plyx
#
# Build:  docker build -t ghcr.io/thereddeveloper/plyx .
# Push:   docker push ghcr.io/thereddeveloper/plyx

FROM alpine:latest

# ── System packages ─────────────────────────────────────────────────────
# gcompat provides glibc compatibility for NDK + SDK binaries on musl.
RUN apk add --no-cache \
    bash \
    curl \
    gcc \
    gcompat \
    git \
    libc-dev \
    musl-dev \
    openjdk17-jdk \
    openssl-dev \
    openssl-libs-static \
    pkgconf \
    unzip \
    wget \
    zip

# ── Rust (latest stable + Android targets) ──────────────────────────────
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain stable --profile minimal && \
    rustup target add \
        aarch64-linux-android \
        armv7-linux-androideabi \
        i686-linux-android \
        x86_64-linux-android && \
    rustc --version

# ── Android SDK ──────────────────────────────────────────────────────────
ENV ANDROID_HOME=/opt/android-sdk \
    JAVA_HOME=/usr/lib/jvm/default-jvm

RUN mkdir -p ${ANDROID_HOME} && \
    cd ${ANDROID_HOME} && \
    wget -q https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip && \
    unzip -q commandlinetools-linux-13114758_latest.zip && \
    rm commandlinetools-linux-13114758_latest.zip && \
    mv cmdline-tools latest && \
    mkdir cmdline-tools && \
    mv latest cmdline-tools/

RUN mkdir -p ~/.android && touch ~/.android/repositories.cfg && \
    yes | ${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager "platform-tools" | grep -v = || true && \
    yes | ${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager "platforms;android-36" | grep -v = || true && \
    yes | ${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager "build-tools;36.0.0-rc5" | grep -v = || true

# ── Android NDK r25 ─────────────────────────────────────────────────────
RUN cd /usr/local && \
    wget -q https://dl.google.com/android/repository/android-ndk-r25-linux.zip && \
    unzip -q android-ndk-r25-linux.zip && \
    rm android-ndk-r25-linux.zip

ENV NDK_HOME=/usr/local/android-ndk-r25

# ── cargo-quad-apk ──────────────────────────────────────────────────────
RUN cargo install --git https://github.com/not-fl3/cargo-quad-apk

# ── Add build-tools to PATH (for apksigner) ─────────────────────────────
ENV PATH="${ANDROID_HOME}/build-tools/36.0.0-rc5:${PATH}"

# ── Workspace ────────────────────────────────────────────────────────────
RUN mkdir -p /root/src
WORKDIR /root/src
