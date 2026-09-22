# Every toolchain this repository builds with, in one image: the same result on
# Windows, Linux and macOS, and nothing to install on the machine but Docker.
#
#   ./build.sh            builds the image on first use, then the packs
#   ./build.sh shell      a shell inside it
#
# Nothing is copied in: the repository is mounted at /work when the image runs,
# so this file only changes when a toolchain does.

FROM devkitpro/devkita64:latest

# The nightly the manager's Rust core is built with (-Zbuild-std).
ARG RUST_NIGHTLY=nightly-2024-10-09
# The .NET SDK is only needed to package .tkcl mods: --build-arg WITH_DOTNET=0
# leaves it out and saves about a gigabyte.
ARG WITH_DOTNET=1

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV DEBIAN_FRONTEND=noninteractive

# devkitPro: the compiler comes with the base image; these are the libraries
# the homebrew links against and the deko3d shader compiler it needs.
RUN dkp-pacman -Syu --noconfirm --needed \
        switch-dev switch-glm switch-curl switch-libarchive uam \
    && rm -rf /opt/devkitpro/pacman/var/cache/pacman/pkg/*

# The rest of the host side: cmake/ninja/make for the homebrew, python for the
# Ryujinx build of the manager, git for the submodules. Meson comes from pip
# rather than apt: Debian's is 1.0, and meson.options needs 1.1.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates cmake curl git ninja-build \
        pkg-config python3 python3-pip unzip zip \
    && pip3 install --break-system-packages --no-cache-dir 'meson>=1.4' \
    && rm -rf /var/lib/apt/lists/*

# Rust, shared by every user of the image rather than hidden in a home
# directory: the nightly above, linkle (ELF -> NRO), and the Skyline toolchain,
# which cargo-skyline builds from a nightly plus skyline-rs' standard library.
ENV RUSTUP_HOME=/opt/rust/rustup \
    CARGO_HOME=/opt/rust/cargo \
    PATH=/opt/rust/cargo/bin:$PATH \
    RUSTUP_MAX_RETRIES=10 \
    CARGO_NET_RETRY=10
# Each download is retried, waiting longer every time: these servers sometimes
# cut a TLS connection short, and github rate-limits by address — cargo-skyline
# asks it which nightly its standard library was built with. A whole image
# should not be lost to one bad minute.
RUN retry() { \
        for attempt in 1 2 3 4 5; do \
            "$@" && return 0; \
            echo "retrying ($attempt): $*" >&2; \
            sleep $((attempt * 30)); \
        done; \
        return 1; \
    }; \
    # A retry of the line below runs after an attempt that got far enough to
    # leave a rustc in PATH, and rustup-init then refuses with "cannot install
    # while Rust is installed": the retry could never succeed.
    export RUSTUP_INIT_SKIP_PATH_CHECK=yes; \
    retry sh -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable" \
    && retry rustup toolchain install "$RUST_NIGHTLY" --profile minimal --component rust-src \
    && retry cargo install linkle --features binaries \
    && retry cargo install cargo-skyline \
    && retry cargo skyline update-std \
    # update-std installs the toolchain but not the linker script the target
    # spec points at; it lives in cargo-skyline's sources, which are still here.
    && cp "$(ls -d "$CARGO_HOME"/registry/src/*/cargo-skyline-*/src/link.T | head -1)" \
          "$CARGO_HOME/skyline/link.T" \
    # The sources cargo downloaded are no longer needed, and they weigh half a
    # gigabyte; what was built from them stays.
    && rm -rf "$CARGO_HOME/registry/cache" "$CARGO_HOME/registry/src" \
    && chmod -R a+rwX /opt/rust

# The .tkcl packager (utils/tkmm-oracle) runs on .NET.
ENV DOTNET_ROOT=/opt/dotnet \
    DOTNET_CLI_TELEMETRY_OPTOUT=1 \
    DOTNET_NOLOGO=1 \
    PATH=/opt/dotnet:$PATH
RUN if [ "$WITH_DOTNET" = 1 ]; then \
        curl -sSL https://dot.net/v1/dotnet-install.sh -o /tmp/dotnet-install.sh \
        && bash /tmp/dotnet-install.sh --channel 10.0 --install-dir /opt/dotnet \
        && rm /tmp/dotnet-install.sh \
        && chmod -R a+rX /opt/dotnet; \
    fi

# The build scripts use a login shell for the devkitPro side, and that resets
# PATH: make sure it still finds these.
RUN printf '%s\n' \
        'export RUSTUP_HOME=/opt/rust/rustup' \
        'export CARGO_HOME=/opt/rust/cargo' \
        'export DOTNET_ROOT=/opt/dotnet' \
        'export PATH=/opt/rust/cargo/bin:/opt/dotnet:$PATH' \
        > /etc/profile.d/totk-toolchains.sh

# Builds run as whoever the host asks for (docker run --user), so the few
# places the build writes to outside the repository have to be open to all:
# .NET and cargo keep caches in HOME. Everything else goes to /work/output.
RUN mkdir -p /work /tmp/totk && chmod 1777 /tmp/totk
ENV HOME=/tmp/totk

WORKDIR /work

CMD ["bash", "-lc", "meson setup output/build && meson compile -C output/build packs"]
