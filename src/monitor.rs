use crate::config::{Alert, AlertTrigger, Config, Target};
use chrono::{DateTime, Utc};
use colored::*;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub target: String,
    pub timestamp: DateTime<Utc>,
    pub success: bool,
    pub status_code: Option<u16>,
    pub response_time: Duration,
    pub error: Option<String>,
    pub cert_expires_days: Option<u32>,
    pub dns_time: Option<Duration>,
    pub connect_time: Option<Duration>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetHealth {
    pub name: String,
    pub url: String,
    pub current_status: HealthStatus,
    pub consecutive_failures: u32,
    pub total_checks: u64,
    pub successful_checks: u64,
    pub uptime_percentage: f64,
    pub avg_response_time: Duration,
    pub min_response_time: Duration,
    pub max_response_time: Duration,
    pub last_check: Option<DateTime<Utc>>,
    pub health_score: f64,
    pub recent_checks: VecDeque<HealthCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

pub struct Monitor {
    config: Config,
    client: Client,
    targets: Arc<Mutex<HashMap<String, TargetHealth>>>,
    running: Arc<AtomicBool>,
    alert_cooldowns: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
}

impl Monitor {
    pub fn new(config: Config) -> Result<Self, Box<dyn std::error::Error>> {
        let client = Client::builder()
            .timeout(Duration::from_secs_f64(config.settings.default_timeout))
            .build()?;

        let mut targets = HashMap::new();
        for target in &config.targets {
            targets.insert(
                target.name.clone(),
                TargetHealth::new(target.clone()),
            );
        }

        Ok(Self {
            config,
            client,
            targets: Arc::new(Mutex::new(targets)),
            running: Arc::new(AtomicBool::new(true)),
            alert_cooldowns: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let running = Arc::clone(&self.running);
        
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })?;

        println!("🚀 Starting HTTP monitor for {} targets...", self.config.targets.len());
        
        let mut handles = Vec::new();
        
        for target in &self.config.targets {
            let target_clone = target.clone();
            let client = self.client.clone();
            let targets = Arc::clone(&self.targets);
            let running = Arc::clone(&self.running);
            let config = self.config.clone();
            let alert_cooldowns = Arc::clone(&self.alert_cooldowns);

            let handle = tokio::spawn(async move {
                Self::monitor_target(target_clone, client, targets, running, config, alert_cooldowns).await;
            });
            
            handles.push(handle);
        }

        // Status reporting task
        let targets_for_status = Arc::clone(&self.targets);
        let running_for_status = Arc::clone(&self.running);
        let status_handle = tokio::spawn(async move {
            while running_for_status.load(Ordering::SeqCst) {
                sleep(Duration::from_secs(30)).await;
                Self::print_status_summary(&targets_for_status);
            }
        });

        handles.push(status_handle);

        // Wait for all tasks
        for handle in handles {
            let _ = handle.await;
        }

        self.print_final_summary();
        Ok(())
    }

    async fn monitor_target(
        target: Target,
        client: Client,
        targets: Arc<Mutex<HashMap<String, TargetHealth>>>,
        running: Arc<AtomicBool>,
        config: Config,
        alert_cooldowns: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
    ) {
        while running.load(Ordering::SeqCst) {
            let start = Instant::now();
            let check = Self::perform_health_check(&target, &client).await;
            
            // Update target health
            {
                let mut targets_lock = targets.lock().unwrap();
                if let Some(health) = targets_lock.get_mut(&target.name) {
                    health.update_with_check(check.clone());
                }
            }

            // Check for alerts
            Self::check_alerts(&target, &check, &config.alerts, &alert_cooldowns).await;

            // Print result
            Self::print_check_result(&target, &check, &config.settings);

            let elapsed = start.elapsed();
            let interval = Duration::from_secs_f64(target.interval_seconds);
            if elapsed < interval {
                sleep(interval - elapsed).await;
            }
        }
    }

    async fn perform_health_check(target: &Target, client: &Client) -> HealthCheck {
        let start = Instant::now();
        
        let method = match target.method.to_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            "HEAD" => Method::HEAD,
            "OPTIONS" => Method::OPTIONS,
            "PATCH" => Method::PATCH,
            _ => Method::GET,
        };

        let mut request_builder = client.request(method, &target.url);

        // Add headers
        for (key, value) in &target.headers {
            request_builder = request_builder.header(key, value);
        }

        // Add random User-Agent if not specified
        if !target.headers.contains_key("User-Agent") && !target.headers.contains_key("user-agent") {
            request_builder = request_builder.header("User-Agent", Self::get_random_user_agent());
        }

        match request_builder.send().await {
            Ok(response) => {
                let response_time = start.elapsed();
                let status_code = response.status().as_u16();
                
                // Check if status code is expected
                let status_ok = if target.expected_status.is_empty() {
                    response.status().is_success()
                } else {
                    target.expected_status.contains(&status_code)
                };

                // Check content if specified
                let mut content_ok = true;
                let mut error = None;

                if let Some(expected_content) = &target.expected_content {
                    match response.text().await {
                        Ok(body) => {
                            content_ok = body.contains(expected_content);
                            if !content_ok {
                                error = Some(format!("Expected content '{}' not found in response", expected_content));
                            }
                        }
                        Err(e) => {
                            content_ok = false;
                            error = Some(format!("Failed to read response body: {}", e));
                        }
                    }
                }

                // Check certificate expiry for HTTPS
                let cert_expires_days = if target.url.starts_with("https://") {
                    Self::check_cert_expiry(&target.url).await
                } else {
                    None
                };

                HealthCheck {
                    target: target.name.clone(),
                    timestamp: Utc::now(),
                    success: status_ok && content_ok,
                    status_code: Some(status_code),
                    response_time,
                    error,
                    cert_expires_days,
                    dns_time: None, // TODO: Implement DNS timing
                    connect_time: None, // TODO: Implement connection timing
                }
            }
            Err(err) => HealthCheck {
                target: target.name.clone(),
                timestamp: Utc::now(),
                success: false,
                status_code: None,
                response_time: start.elapsed(),
                error: Some(err.to_string()),
                cert_expires_days: None,
                dns_time: None,
                connect_time: None,
            },
        }
    }

    async fn check_cert_expiry(url: &str) -> Option<u32> {
        // Simple certificate expiry check - in a real implementation you'd use rustls/webpki
        // For now, we'll skip this complex implementation
        None
    }

    async fn check_alerts(
        target: &Target,
        check: &HealthCheck,
        alerts: &[Alert],
        alert_cooldowns: &Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
    ) {
        for alert in alerts {
            let should_alert = Self::should_trigger_alert(alert, target, check);
            
            if should_alert {
                let now = Utc::now();
                let cooldown_key = format!("{}:{}", alert.name, target.name);
                
                let should_send = {
                    let mut cooldowns = alert_cooldowns.lock().unwrap();
                    if let Some(last_sent) = cooldowns.get(&cooldown_key) {
                        let cooldown_duration = chrono::Duration::minutes(alert.cooldown_minutes as i64);
                        now.signed_duration_since(*last_sent) > cooldown_duration
                    } else {
                        true
                    }
                };

                if should_send {
                    Self::send_alert(alert, target, check).await;
                    let mut cooldowns = alert_cooldowns.lock().unwrap();
                    cooldowns.insert(cooldown_key, now);
                }
            }
        }
    }

    fn should_trigger_alert(alert: &Alert, target: &Target, check: &HealthCheck) -> bool {
        // This is simplified - in reality you'd track state over time
        for trigger in &alert.trigger_on {
            match trigger {
                AlertTrigger::ResponseTimeMs(threshold) => {
                    if check.response_time.as_millis() as u64 > *threshold {
                        return true;
                    }
                }
                AlertTrigger::CertExpiringDays(days) => {
                    if let Some(cert_days) = check.cert_expires_days {
                        if cert_days <= *days {
                            return true;
                        }
                    }
                }
                _ => {} // ConsecutiveFailures and HealthScoreBelow need more state tracking
            }
        }
        false
    }

    async fn send_alert(alert: &Alert, target: &Target, check: &HealthCheck) {
        let payload = serde_json::json!({
            "text": format!("🚨 Alert: {} - {}", alert.name, target.name),
            "attachments": [{
                "color": "danger",
                "fields": [
                    {"title": "Target", "value": target.name, "short": true},
                    {"title": "URL", "value": target.url, "short": true},
                    {"title": "Status", "value": check.status_code.map_or("Error".to_string(), |c| c.to_string()), "short": true},
                    {"title": "Response Time", "value": format!("{}ms", check.response_time.as_millis()), "short": true},
                    {"title": "Error", "value": check.error.as_deref().unwrap_or("N/A"), "short": false}
                ]
            }]
        });

        let client = Client::new();
        let _ = client.post(&alert.webhook_url)
            .json(&payload)
            .send()
            .await;
    }

    fn print_check_result(target: &Target, check: &HealthCheck, settings: &crate::config::Settings) {
        if !settings.enable_colors {
            colored::control::set_override(false);
        }

        let status_color = if check.success {
            "✓".green()
        } else {
            "✗".red()
        };

        let status_code_str = check.status_code
            .map_or("ERROR".red().to_string(), |code| {
                match code {
                    200..=299 => code.to_string().green().to_string(),
                    300..=399 => code.to_string().yellow().to_string(),
                    _ => code.to_string().red().to_string(),
                }
            });

        let time_str = {
            let ms = check.response_time.as_millis();
            match ms {
                0..=200 => format!("{}ms", ms).green().to_string(),
                201..=1000 => format!("{}ms", ms).yellow().to_string(),
                _ => format!("{}ms", ms).red().to_string(),
            }
        };

        println!("[{}] {} {} | {} | {}",
                 check.timestamp.format("%H:%M:%S"),
                 status_color,
                 target.name.bold(),
                 status_code_str,
                 time_str);

        if let Some(error) = &check.error {
            println!("    Error: {}", error.red());
        }
    }

    fn print_status_summary(targets: &Arc<Mutex<HashMap<String, TargetHealth>>>) {
        let targets_lock = targets.lock().unwrap();
        println!("\n📊 Status Summary:");
        println!("{:<20} {:<10} {:<10} {:<15} {:<10}", "Target", "Status", "Uptime", "Avg Response", "Health");
        println!("{}", "─".repeat(75));
        
        for health in targets_lock.values() {
            let status = match health.current_status {
                HealthStatus::Healthy => "Healthy".green(),
                HealthStatus::Degraded => "Degraded".yellow(),
                HealthStatus::Unhealthy => "Unhealthy".red(),
                HealthStatus::Unknown => "Unknown".white(),
            };

            println!("{:<20} {:<10} {:<10.1}% {:<15}ms {:<10.1}",
                     health.name,
                     status,
                     health.uptime_percentage,
                     health.avg_response_time.as_millis(),
                     health.health_score * 100.0);
        }
        println!();
    }

    fn print_final_summary(&self) {
        println!("\n🏁 Final Summary:");
        Self::print_status_summary(&self.targets);
    }

    fn get_random_user_agent() -> &'static str {
        crate::HttpPinger::get_random_user_agent()
    }
}

impl TargetHealth {
    fn new(target: Target) -> Self {
        Self {
            name: target.name,
            url: target.url,
            current_status: HealthStatus::Unknown,
            consecutive_failures: 0,
            total_checks: 0,
            successful_checks: 0,
            uptime_percentage: 0.0,
            avg_response_time: Duration::from_millis(0),
            min_response_time: Duration::from_millis(u64::MAX),
            max_response_time: Duration::from_millis(0),
            last_check: None,
            health_score: 1.0,
            recent_checks: VecDeque::with_capacity(100),
        }
    }

    fn update_with_check(&mut self, check: HealthCheck) {
        self.total_checks += 1;
        self.last_check = Some(check.timestamp);

        if check.success {
            self.successful_checks += 1;
            self.consecutive_failures = 0;
        } else {
            self.consecutive_failures += 1;
        }

        // Update response time stats
        if check.response_time < self.min_response_time {
            self.min_response_time = check.response_time;
        }
        if check.response_time > self.max_response_time {
            self.max_response_time = check.response_time;
        }

        // Calculate average response time
        let total_time_ms = (self.avg_response_time.as_millis() as u64 * (self.total_checks - 1)) + check.response_time.as_millis() as u64;
        self.avg_response_time = Duration::from_millis(total_time_ms / self.total_checks);

        // Update uptime percentage
        self.uptime_percentage = (self.successful_checks as f64 / self.total_checks as f64) * 100.0;

        // Update current status
        self.current_status = if self.consecutive_failures == 0 {
            if self.uptime_percentage >= 99.0 {
                HealthStatus::Healthy
            } else if self.uptime_percentage >= 95.0 {
                HealthStatus::Degraded
            } else {
                HealthStatus::Unhealthy
            }
        } else if self.consecutive_failures >= 3 {
            HealthStatus::Unhealthy
        } else {
            HealthStatus::Degraded
        };

        // Calculate health score (0.0 to 1.0)
        let uptime_score = self.uptime_percentage / 100.0;
        let response_time_score = if self.avg_response_time.as_millis() <= 500 {
            1.0
        } else if self.avg_response_time.as_millis() <= 2000 {
            0.8
        } else if self.avg_response_time.as_millis() <= 5000 {
            0.5
        } else {
            0.2
        };
        
        self.health_score = (uptime_score * 0.7) + (response_time_score * 0.3);

        // Store recent checks (keep last 100)
        self.recent_checks.push_back(check);
        if self.recent_checks.len() > 100 {
            self.recent_checks.pop_front();
        }
    }
}