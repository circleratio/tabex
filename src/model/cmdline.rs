//! Commands typed after ":" in the command line (requirement 6.2).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmdLine {
    /// `:sh` — open a shell in the folder of the active tab.
    Shell,
}

/// `Ok(None)` for an empty line; `Err(text)` for an unknown command.
pub fn parse(s: &str) -> Result<Option<CmdLine>, String> {
    match s.trim() {
        "" => Ok(None),
        "sh" => Ok(Some(CmdLine::Shell)),
        other => Err(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        assert_eq!(parse("sh"), Ok(Some(CmdLine::Shell)));
        assert_eq!(parse("  sh \t"), Ok(Some(CmdLine::Shell)));
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("   "), Ok(None));
        assert_eq!(parse("foo"), Err("foo".to_string()));
        // Case-sensitive like vi.
        assert_eq!(parse("SH"), Err("SH".to_string()));
        assert_eq!(parse("sh x"), Err("sh x".to_string()));
    }
}
