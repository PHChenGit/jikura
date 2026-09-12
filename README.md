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

## Search

Press `/` on either page and type to filter the list immediately. Name matching
ignores case and allows gaps: `ngx` matches `nginx`. IDs require a contiguous
prefix: `6d33` matches `6d336809823`, while `6809` and `6d38` do not. The
`sha256:` prefix is optional when searching image IDs. Containers match any
container name or ID, plus their image name or ID. Images match their ID and
all repository tags and digests, including tags beyond the one shown in the
table. Both full and shortened IDs work.

Press `Enter` or `Esc` to finish editing and navigate the matching rows.
Press `/` to edit again, `Backspace` to delete, or `Ctrl-U` while editing to
clear the query. `Esc` outside the input also clears the search. Clearing
immediately restores the full list under the current `a` (all) setting.
Each page remembers its own query across tab switches and refreshes.

## Configuration

Jikura reads `$XDG_CONFIG_HOME/jikura/config.toml`, or
`~/.config/jikura/config.toml` when `XDG_CONFIG_HOME` is unset. To use another
file, run `jikura --config /path/to/config.toml`.

```toml
[settings]
GITLAB_IMAGE_API = ""

[CONTAINERS.my-container]
USER = "root"
SHELL = "bash"

[CONTAINERS."app.production"]
USER = "app"
SHELL = "/bin/sh"
```

`GITLAB_IMAGE_API` stores the GitLab registry API endpoint; an empty value
leaves it unset. `GITLAB_IAMGE_API` is accepted as an alias. This configuration
change does not yet fetch or pull remote images.

Each section under `CONTAINERS` matches an exact container name, without a
leading slash. Existing flat sections such as `[my-container]` also work in
files that have neither `[settings]` nor `[CONTAINERS]`. When adding either
new section, move all container sections under `CONTAINERS` as shown above.
Omitted fields and containers default to `root` and `bash`. `USER` names a user inside
the container (a UID or `user:group` also works). `SHELL` is a single executable
name or path inside the container; it is not a command line. Use `sh` or
`/bin/sh` for images without Bash. Values are passed as literal arguments.

Configuration is read at startup. A missing default file is allowed; an invalid
file or a missing explicitly supplied file reports an error before opening the
UI. See [config.example.toml](config.example.toml) for a complete example.

## Development

See [.devcontainer/README.md](.devcontainer/README.md) for the Rust development
environment. Run `cargo test` and `cargo clippy --all-targets -- -D warnings`.
