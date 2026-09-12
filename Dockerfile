FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Docker CLI only -- deliberately NO daemon. The container talks to the
# host's rootless podman socket, which serves the Docker-compatible API,
# so `docker` here drives the same engine your TUI drives. Useful for
# checking kune's output against a known-good client.
RUN install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/debian/gpg \
         -o /etc/apt/keyrings/docker.asc \
    && chmod a+r /etc/apt/keyrings/docker.asc \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian bookworm stable" \
         > /etc/apt/sources.list.d/docker.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends docker-ce-cli \
    && rm -rf /var/lib/apt/lists/*
