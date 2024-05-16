FROM archlinux as build

WORKDIR /app

RUN pacman -Syyu --noconfirm && \
    pacman -S --noconfirm git make clang rust && \
    git clone https://github.com/near/nearcore && \
    cd nearcore && \
    make sandbox

FROM archlinux

WORKDIR /app

RUN pacman -Syyu --noconfirm && \
    pacman -S --noconfirm rust openssl pkg-config

COPY --from=build /app/nearcore/target/debug/near-sandbox /usr/bin/near-sandbox
