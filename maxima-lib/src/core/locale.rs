use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Locale {
    EnUs,
}

impl Locale {
    pub fn short_str(&self) -> &'static str {
        match self {
            Locale::EnUs => "en",
        }
    }

    pub fn full_str(&self) -> &'static str {
        match self {
            Locale::EnUs => "en_US",
        }
    }
}
