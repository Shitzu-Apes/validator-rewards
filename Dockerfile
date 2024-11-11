FROM archlinux as build

WORKDIR /app

RUN pacman -Syyu --noconfirm && \
    pacman -S --noconfirm git make clang rust && \
    git clone https://github.com/near/nearcore && \
    cd nearcore && \
    git reset --hard 2.3.0 && \
    make sandbox

FROM archlinux

WORKDIR /app

RUN pacman -Syyu --noconfirm && \
    pacman -S --noconfirm openssl pkg-config gcc && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs/ | sh -s -- --default-toolchain=1.81.0 -y
ENV PATH="$PATH:/root/.cargo/bin"

COPY --from=build /app/nearcore/target/debug/near-sandbox /usr/bin/near-sandbox
