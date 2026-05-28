use clap::Parser;

use crate::NcalError;

#[derive(Parser, Debug)]
#[command( author, version, about, long_about = None)]
pub struct Cli {
    /// [[[day] month] year]]
    #[arg(value_parser = clap::value_parser!(u32))]
    args: Vec<u32>,
}

#[allow(unused)]
impl Cli {
    pub fn parse_args(&self) -> Result<(Option<u8>, Option<u8>, Option<u32>), NcalError> {
        let mut day: Option<u8> = None;
        let mut month: Option<u8> = None;
        let mut year: Option<u32> = None;

        match self.args.len() {
            1 => {
                year = Some(self.args.get(0).cloned().unwrap());
            }
            2 => {
                month = Some(self.args.get(0).cloned().unwrap() as u8);
                year = Some(self.args.get(1).cloned().unwrap());
            }
            3 => {
                day = Some(self.args.get(0).cloned().unwrap() as u8);
                month = Some(self.args.get(1).cloned().unwrap() as u8);
                year = Some(self.args.get(2).cloned().unwrap());
            }
            _ => {
                return Err(NcalError::InvalidArgumentLength {
                    arg: format!("{:?}", self.args.len()),
                });
            }
        }

        Ok((day, month, year))
    }
}
