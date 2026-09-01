use clap::Parser;

/// Browse local docker/podman images and containers.
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "jikura", version, about)]
pub struct Args {
    /// Show stopped containers and intermediate images too.
    #[arg(short, long)]
    pub all: bool,

    /// Engine endpoint; sets DOCKER_HOST for this run.
    #[arg(long, value_name = "URL")]
    pub host: Option<String>,

    /// Seconds between automatic refreshes.
    #[arg(short, long, value_name = "SECS", default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub refresh: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::parse_from(std::iter::once("jikura").chain(args.iter().copied()))
    }

    #[test]
    fn the_defaults_are_running_containers_and_a_two_second_refresh() {
        let args = parse(&[]);
        assert!(!args.all);
        assert_eq!(args.refresh, 2);
        assert_eq!(args.host, None);
    }

    #[test]
    fn all_and_refresh_have_short_forms() {
        let args = parse(&["-a", "-r", "10"]);
        assert!(args.all);
        assert_eq!(args.refresh, 10);
    }

    #[test]
    fn a_host_can_be_pointed_at_a_specific_socket() {
        let args = parse(&["--host", "unix:///run/user/1000/podman/podman.sock"]);
        assert_eq!(
            args.host.as_deref(),
            Some("unix:///run/user/1000/podman/podman.sock")
        );
    }

    #[test]
    fn a_zero_refresh_is_rejected_rather_than_spinning_the_engine() {
        assert!(
            Args::try_parse_from(["jikura", "--refresh", "0"]).is_err(),
            "a zero-second refresh would hammer the engine"
        );
    }
}
