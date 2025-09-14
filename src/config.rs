use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub targets: Vec<Target>,
    pub settings: Settings,
    #[serde(default)]
    pub alerts: Vec<Alert>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Target {
    pub name: String,
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub expected_status: Vec<u16>,
    #[serde(default)]
    pub expected_content: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: f64,
    #[serde(default = "default_interval")]
    pub interval_seconds: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Settings {
    #[serde(default = "default_interval")]
    pub default_interval: f64,
    #[serde(default = "default_timeout")]
    pub default_timeout: f64,
    #[serde(default = "default_max_failures")]
    pub max_consecutive_failures: u32,
    #[serde(default = "default_health_window")]
    pub health_check_window_minutes: u32,
    #[serde(default)]
    pub output_format: OutputFormat,
    #[serde(default)]
    pub enable_colors: bool,
    #[serde(default)]
    pub log_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    pub name: String,
    pub webhook_url: String,
    pub trigger_on: Vec<AlertTrigger>,
    #[serde(default = "default_cooldown")]
    pub cooldown_minutes: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertTrigger {
    ConsecutiveFailures(u32),
    ResponseTimeMs(u64),
    HealthScoreBelow(f64),
    CertExpiringDays(u32),
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Pretty,
    Json,
    Csv,
    Prometheus,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_timeout() -> f64 {
    10.0
}

fn default_interval() -> f64 {
    60.0
}

fn default_max_failures() -> u32 {
    3
}

fn default_health_window() -> u32 {
    60
}

fn default_cooldown() -> u32 {
    30
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_interval: default_interval(),
            default_timeout: default_timeout(),
            max_consecutive_failures: default_max_failures(),
            health_check_window_minutes: default_health_window(),
            output_format: OutputFormat::default(),
            enable_colors: true,
            log_file: None,
        }
    }
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    pub fn example() -> Self {
        Config {
            targets: vec![
                Target {
                    name: "Production API".to_string(),
                    url: "https://api.example.com/health".to_string(),
                    method: "GET".to_string(),
                    headers: HashMap::new(),
                    expected_status: vec![200],
                    expected_content: Some("\"status\":\"ok\"".to_string()),
                    timeout_seconds: 5.0,
                    interval_seconds: 30.0,
                },
                Target {
                    name: "Main Website".to_string(),
                    url: "https://example.com".to_string(),
                    method: "GET".to_string(),
                    headers: HashMap::new(),
                    expected_status: vec![200, 301, 302],
                    expected_content: None,
                    timeout_seconds: 10.0,
                    interval_seconds: 60.0,
                },
            ],
            settings: Settings::default(),
            alerts: vec![
                Alert {
                    name: "Slack Alerts".to_string(),
                    webhook_url: "https://hooks.slack.com/services/YOUR/WEBHOOK/URL".to_string(),
                    trigger_on: vec![
                        AlertTrigger::ConsecutiveFailures(3),
                        AlertTrigger::ResponseTimeMs(5000),
                        AlertTrigger::CertExpiringDays(7),
                    ],
                    cooldown_minutes: 30,
                },
            ],
        }
    }
}