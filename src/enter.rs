//! Foreground shell command. Arguments are passed literally, never via a host shell.

use std::process::{Command, Stdio};

use crate::config::ShellConfig;
use crate::domain::ContainerId;
use crate::engine::EngineError;

pub fn command(endpoint: &str, container: &ContainerId, settings: &ShellConfig) -> Command {
    let mut command = Command::new("docker");
    // Pin the same endpoint as the API, even if the CLI has an active context.
    command.env_remove("DOCKER_CONTEXT");
    command.args([
        "--host",
        endpoint,
        "exec",
        "-u",
        &settings.user,
        "-it",
        container.as_str(),
        &settings.shell,
    ]);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

pub fn run(
    endpoint: &str,
    container: &ContainerId,
    settings: &ShellConfig,
) -> Result<(), EngineError> {
    let status = command(endpoint, container, settings)
        .status()
        .map_err(|err| EngineError::Other {
            message: format!(
                "could not launch docker exec (the Docker CLI must be on PATH): {err}"
            ),
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(EngineError::Other {
            message: format!("docker exec exited with {status}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_full_id_endpoint_and_literal_user_and_shell() {
        let settings = ShellConfig {
            user: "app:staff".into(),
            shell: "/bin/sh; echo unsafe".into(),
        };
        let cmd = command(
            "unix:///run/podman.sock",
            &ContainerId::from("abcdef0123456789".to_owned()),
            &settings,
        );
        assert_eq!(cmd.get_program(), "docker");
        let args: Vec<_> = cmd.get_args().map(|arg| arg.to_str().unwrap()).collect();
        assert_eq!(
            args,
            [
                "--host",
                "unix:///run/podman.sock",
                "exec",
                "-u",
                "app:staff",
                "-it",
                "abcdef0123456789",
                "/bin/sh; echo unsafe"
            ]
        );
    }
}
