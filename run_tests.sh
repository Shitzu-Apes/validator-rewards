#!/usr/bin/env bash

set -e

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"

NAME="near_runner"

if docker ps -a --format '{{.Names}}' | grep -Eq "^${NAME}\$"; then
    echo "Container exists"
else
docker create \
    --mount type=bind,source=$DIR,target=/app \
    --cap-add=SYS_PTRACE --security-opt seccomp=unconfined \
    --name=$NAME \
    -it \
    tarnadas/near-sandbox \
    /bin/bash
fi

docker start $NAME
docker exec $NAME /bin/bash -c \
    "cargo test $1 -- --nocapture --test-threads=1"

# docker stop $NAME
# docker rm $NAME
