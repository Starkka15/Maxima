use serde::{Deserialize, Serialize};

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MaximaSetting {
    #[default]
    IsIgoEnabled,
    IsIgoAvailable,
    Environment
}
