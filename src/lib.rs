pub mod cli;
pub mod config;

use thiserror::Error;

#[derive(Debug, Error)]
#[allow(unused)]
pub enum NcalError {
    #[error("Invalid argument; expected 1-3 arguments, got {arg}")]
    InvalidArgumentLength { arg: String },

    #[error("Invalid month; choose between 1-12")]
    InvalidMonth,

    #[error("Invalid day")]
    InvalidDay,
}
