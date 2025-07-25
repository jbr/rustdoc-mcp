use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Controls the verbosity level of documentation display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Verbosity {
    Minimal,
    Brief,
    Full,
}

impl Default for Verbosity {
    fn default() -> Self {
        Self::Brief
    }
}
