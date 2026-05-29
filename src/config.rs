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

    pub fn validate(&self) -> Result<(), NcalError> {
        if let Some(month) = self.month {
            if month > 12 {
                return Err(NcalError::InvalidMonth);
            }
        }

        if let Some(day) = self.day {
            if day > 31 {
                return Err(NcalError::InvalidDay);
            }
        }

        Ok(())
    }
}
