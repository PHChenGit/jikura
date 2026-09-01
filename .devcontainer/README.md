# Dev container

Rust toolchain + a Docker-compatible client, wired to the container engine
running on your host so the TUI can manage real images and containers.

## Opening it

VS Code: **Dev Containers: Reopen in Container**. Your user settings already
point Dev Containers at rootless podman:

```
dev.containers.dockerPath          = podman
dev.containers.dockerSocketPath    = /run/user/1000/podman/podman.sock
dev.containers.dockerComposePath   = podman-compose
```

CLI alternative: `npm i -g @devcontainers/cli`, then
`devcontainer up --workspace-folder .` and
`devcontainer exec --workspace-folder . zsh`.

## What "docker" means on this host

`/usr/bin/docker` is podman-docker emulation over **podman 6.1.0** -- there is
no Docker daemon here. Podman exposes a Docker-compatible REST API on its
socket, so `bollard`, the `docker` CLI, and anything else speaking the Docker
API work unchanged against it.

Two traps this config works around:

1. **`/var/run/docker.sock` is a dead end.** It symlinks to
   `/run/podman/podman.sock`, the *system* socket, and `podman.socket` is
   inactive at the system level. The live socket is the **rootless** one at
   `$XDG_RUNTIME_DIR/podman/podman.sock` (`/run/user/1000/podman/podman.sock`).
   That is what gets mounted, and it lands on `/var/run/docker.sock` *inside*
   the container so no code needs a custom path.

2. **User namespaces.** Rootless podman maps container uids into your subuid
   range by default, so a bind-mounted socket owned by host uid 1000 is
   unreadable from inside. `--userns=keep-id` maps host uid 1000 to container
   uid 1000 (`vscode`), which fixes both socket access and file ownership on
   the mounted sources.

If `docker version` fails inside the container, check in this order:

```sh
systemctl --user is-active podman.socket     # expect: active
systemctl --user enable --now podman.socket  # if it is not
ls -ln /run/user/$(id -u)/podman/podman.sock # expect owner 1000
echo "$XDG_RUNTIME_DIR"                      # must be set when VS Code launches
```

The mount source falls back to `/run/user/1000` when `XDG_RUNTIME_DIR` is
unset in the environment VS Code inherits. If your uid is ever not 1000, edit
that fallback in `devcontainer.json`.

## API version, when you wire up bollard

The mounted socket reports **Docker API 1.44** (podman 6.1.0), while a modern
`docker` CLI advertises 29.x. Verified from inside the container:

```
client=29.7.2 server=6.1.0 api=1.44
```

The CLI negotiates this automatically. `bollard` does not always: prefer
`Docker::connect_with_socket_defaults()` followed by a `negotiate_version()`
call (or pin `ClientVersion { major_api_version: 1, minor_api_version: 44 }`)
rather than assuming the newest API. Endpoints Docker has but podman lacks --
Swarm, some `/system` fields -- will 404 or return partial structs, so treat
missing fields as absent rather than as errors.

## Running the TUI

Run it in the integrated terminal (or `devcontainer exec`), not in a VS Code
task or debug console -- TUIs need a real tty:

```sh
cargo run
```

`TERM=xterm-256color` and `COLORTERM=truecolor` are preset so crossterm emits
256-colour and truecolour sequences and mouse reporting works. For a
full-screen debugger session, prefer `rust-gdb`/`lldb` in a second terminal
over the CodeLLDB console, which is not a tty.

## Caches

`/usr/local/cargo/registry` and `/usr/local/cargo/git` are named volumes
(`kune-cargo-registry`, `kune-cargo-git`), so crate downloads persist across
rebuilds. `target/` is intentionally left on the bind mount so build output
stays visible from the host. To reset:

```sh
podman volume rm kune-cargo-registry kune-cargo-git
```

## Scope of access

Mounting the socket gives everything in this container full control of your
**rootless** podman: it can create, stop, and remove your containers and
images. That is unavoidable when developing a tool that manages real container
state. It cannot reach root-owned containers. If you want to exercise
destructive paths (`prune`, `rmi`, `kill`) against throwaway state instead,
swap the socket mount for the docker-in-docker feature.
