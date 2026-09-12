# Jikura

A terminal UI for local Docker and Podman containers and images.

Run `jikura`, select a container, and press `Enter` or `x` to open its action
menu. Choose **Enter** to open an interactive shell. This action is available
only for running containers. Type `exit` or press `Ctrl-D` to return to Jikura.

Entering requires a Docker-compatible `docker` CLI on `PATH`. Jikura runs
`docker --host ENDPOINT exec -u USER -it CONTAINER_ID SHELL`, using the same
engine endpoint as the container page (`--host`, `DOCKER_HOST`, or the default
socket). The shell controls the terminal while it runs, including `Ctrl-C`
and terminal resizing.

## Container shell configuration

Jikura reads `$XDG_CONFIG_HOME/jikura/config.toml`, or
`~/.config/jikura/config.toml` when `XDG_CONFIG_HOME` is unset. To use another
file, run `jikura --config /path/to/config.toml`.

```toml
[my-container]
USER = "root"
SHELL = "bash"

["app.production"]
USER = "app"
SHELL = "/bin/sh"
```

Each section matches an exact container name, without a leading slash. Omitted
fields and containers default to `root` and `bash`. `USER` names a user inside
the container (a UID or `user:group` also works). `SHELL` is a single executable
name or path inside the container; it is not a command line. Use `sh` or
`/bin/sh` for images without Bash. Values are passed as literal arguments.

Configuration is read at startup. A missing default file is allowed; an invalid
file or a missing explicitly supplied file reports an error before opening the
UI. See [config.example.toml](config.example.toml) for a complete example.

## Development

See [.devcontainer/README.md](.devcontainer/README.md) for the Rust development
environment. Run `cargo test` and `cargo clippy --all-targets -- -D warnings`.
