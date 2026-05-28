use crate::{NcalError, cli::Cli};

#[derive(Debug, Clone)]
pub struct Config {
    pub day: Option<u8>,
    pub month: Option<u8>,
    pub year: Option<u32>,
}

impl Config {
    pub fn from_cli(cli: &Cli) -> Result<Self, NcalError> {
        match cli.parse_args() {
            Ok((day, month, year)) => Ok(Self { day, month, year }),
            Err(e) => Err(e),
        }
    }
}
