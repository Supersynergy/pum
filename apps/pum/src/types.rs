use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub manager: String,
    pub name: String,
    pub installed: String,
    pub latest: Option<String>,
    pub status: Option<String>,
    pub source: String,
    pub checked_at: String,
}

impl Package {
    pub fn new(manager: &str, name: &str, installed: &str, source: &str) -> Self {
        Package {
            manager: manager.to_string(),
            name: name.to_string(),
            installed: installed.to_string(),
            latest: None,
            status: Some("unknown".to_string()),
            source: source.to_string(),
            checked_at: Utc::now().to_rfc3339(),
        }
    }

    pub fn outdated(
        manager: &str,
        name: &str,
        installed: &str,
        latest: &str,
        source: &str,
    ) -> Self {
        Package {
            manager: manager.to_string(),
            name: name.to_string(),
            installed: installed.to_string(),
            latest: Some(latest.to_string()),
            status: Some("outdated".to_string()),
            source: source.to_string(),
            checked_at: Utc::now().to_rfc3339(),
        }
    }
}
