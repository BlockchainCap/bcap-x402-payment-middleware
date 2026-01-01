#!/bin/bash

# Get the parent directory (one level up from current workspace)
PARENT_DIR=$(dirname "$PWD")
PROJECT_NAME=$(basename "$PWD")

docker run --rm -it --platform=linux/amd64 \
  -v "$PARENT_DIR":/work -w /work/$PROJECT_NAME \
  amazonlinux:2023 \
  bash -lc '
    set -e
    dnf install -y gcc gcc-c++ make cmake pkgconf-pkg-config \
      openssl openssl-devel \
      clang clang-libs llvm llvm-devel \
      git perl ca-certificates
    dnf install -y --allowerasing curl
    curl https://sh.rustup.rs -sSf | sh -s -- -y
    . "$HOME/.cargo/env"
    cargo build -p payment-gateway --release
  '

# use scp to copy the binary to the remote server
# scp -i <pem file> target/release/payment-gateway ec2-user@IP:/home/ec2-user/