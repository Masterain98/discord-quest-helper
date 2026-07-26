use discord_cdp_launch_core::{parse_discord_channel, DiscordChannel, DEFAULT_CDP_PORT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CliOptions {
    pub port: u16,
    pub channel: Option<DiscordChannel>,
    pub restart: bool,
    pub status: bool,
    pub help: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            port: DEFAULT_CDP_PORT,
            channel: None,
            restart: false,
            status: false,
            help: false,
        }
    }
}

pub(crate) fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    let mut options = CliOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--help" | "-h" => options.help = true,
            "--port" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--port requires a value".to_string())?;
                let port = value
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid --port value: {value}"))?;
                if port == 0 {
                    return Err("--port must be between 1 and 65535".to_string());
                }
                options.port = port;
            }
            "--channel" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--channel requires a value".to_string())?;
                options.channel =
                    parse_discord_channel(Some(value)).map_err(|error| error.to_string())?;
            }
            "--restart" => options.restart = true,
            "--status" => options.status = true,
            unknown => {
                return Err(format!("Unknown argument: {unknown}\n\n{}", help_text()));
            }
        }
        index += 1;
    }
    Ok(options)
}

pub(crate) fn help_text() -> &'static str {
    "Usage:
  discord-cdp-launcher --port 9223 --channel auto
  discord-cdp-launcher --port 9223 --channel stable
  discord-cdp-launcher --port 9223 --restart
  discord-cdp-launcher --status --port 9223

Options:
  --port <port>                 CDP debugging port. Defaults to 9223.
  --channel <auto|stable|ptb|canary>
                                Discord channel to launch. Defaults to auto.
  --restart                     Close the selected Discord client before launching.
  --status                      Check whether CDP is already available.
  --help, -h                    Show this help."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn keeps_supported_cli_parameters() {
        let parsed = parse_args(values(&[
            "--port",
            "9444",
            "--channel",
            "discord-ptb",
            "--restart",
            "--status",
        ]))
        .unwrap();
        assert_eq!(parsed.port, 9444);
        assert_eq!(parsed.channel, Some(DiscordChannel::Ptb));
        assert!(parsed.restart);
        assert!(parsed.status);
    }

    #[test]
    fn defaults_and_help_remain_compatible() {
        assert_eq!(parse_args(Vec::new()).unwrap(), CliOptions::default());
        assert!(parse_args(values(&["--help"])).unwrap().help);
        assert!(help_text().contains("--channel <auto|stable|ptb|canary>"));
    }

    #[test]
    fn rejects_invalid_arguments_and_ports() {
        assert!(parse_args(values(&["--port", "0"])).is_err());
        assert!(parse_args(values(&["--port"])).is_err());
        assert!(parse_args(values(&["--unknown"])).is_err());
    }
}
