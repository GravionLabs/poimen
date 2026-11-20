//! The command line of the ssh that reaches a machine's server.

/// The arguments of ssh for `target`: no terminal, never a prompt, bounded connect, a dead link
/// noticed within 45 s, and the server of this build's channel in stdio mode.
#[cfg(test)]
pub fn ssh_args(target: &str, server: &str) -> Vec<String> {
    ssh_args_with(target, server, &[])
}

/// [`ssh_args`] with extra ssh options (the connection sharing of #1012) before the target.
pub fn ssh_args_with(target: &str, server: &str, extra: &[String]) -> Vec<String> {
    let fixed = [
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
    ];
    fixed
        .iter()
        .map(|s| (*s).to_owned())
        .chain(extra.iter().cloned())
        .chain(["--", target, server, "stdio"].map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_connection_options_come_before_the_target() {
        let extra = vec!["-o".to_owned(), "ControlMaster=auto".to_owned()];
        let args = ssh_args_with("box", "poimen-server", &extra);
        let at = |x: &str| args.iter().position(|a| a == x).unwrap();
        assert!(at("ControlMaster=auto") < at("--"));
        assert_eq!(&args[at("--") + 1..], ["box", "poimen-server", "stdio"]);
        assert_eq!(
            ssh_args_with("box", "poimen-server", &[]),
            ssh_args("box", "poimen-server")
        );
    }
}
