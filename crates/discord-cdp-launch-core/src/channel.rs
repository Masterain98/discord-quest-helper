use crate::LaunchError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "lowercase")
)]
pub enum DiscordChannel {
    Stable,
    Ptb,
    Canary,
}

impl DiscordChannel {
    pub const ALL: [Self; 3] = [Self::Stable, Self::Ptb, Self::Canary];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Ptb => "ptb",
            Self::Canary => "canary",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Ptb => "PTB",
            Self::Canary => "Canary",
        }
    }
}

pub fn parse_discord_channel(value: Option<&str>) -> Result<Option<DiscordChannel>, LaunchError> {
    let Some(value) = value else {
        return Ok(None);
    };

    match value.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(None),
        "stable" | "discord" => Ok(Some(DiscordChannel::Stable)),
        "ptb" | "discordptb" | "discord-ptb" => Ok(Some(DiscordChannel::Ptb)),
        "canary" | "discordcanary" | "discord-canary" => Ok(Some(DiscordChannel::Canary)),
        other => Err(LaunchError::UnsupportedChannel(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_supported_channel_aliases() {
        let cases = [
            ("stable", Some(DiscordChannel::Stable)),
            ("discord", Some(DiscordChannel::Stable)),
            ("ptb", Some(DiscordChannel::Ptb)),
            ("discordptb", Some(DiscordChannel::Ptb)),
            ("discord-ptb", Some(DiscordChannel::Ptb)),
            ("canary", Some(DiscordChannel::Canary)),
            ("discordcanary", Some(DiscordChannel::Canary)),
            ("discord-canary", Some(DiscordChannel::Canary)),
            ("auto", None),
            ("", None),
            ("  AUTO  ", None),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_discord_channel(Some(input)).unwrap(), expected);
        }
        assert_eq!(parse_discord_channel(None).unwrap(), None);
    }

    #[test]
    fn rejects_unsupported_channel() {
        assert!(matches!(
            parse_discord_channel(Some("nightly")),
            Err(LaunchError::UnsupportedChannel(value)) if value == "nightly"
        ));
    }
}
