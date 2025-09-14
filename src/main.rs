mod config;
mod monitor;

use clap::{Parser, Subcommand};
use colored::*;
use config::{Config, Target};
use monitor::Monitor;
use rand::seq::SliceRandom;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[derive(Parser, Debug)]
#[command(name = "httpping")]
#[command(about = "Advanced HTTP monitoring and ping utility")]
#[command(version = "0.2.0")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    
    // Legacy single-URL mode (when no subcommand is used)
    #[arg(help = "URL to ping (legacy mode)")]
    url: Option<String>,

    #[arg(short = 'c', long = "count", help = "Number of requests (default: infinite)")]
    count: Option<u64>,

    #[arg(short = 'i', long = "interval", help = "Interval between requests in seconds", default_value = "1.0")]
    interval: f64,

    #[arg(short = 't', long = "timeout", help = "Request timeout in seconds", default_value = "10.0")]
    timeout: f64,

    #[arg(short = 'm', long = "method", help = "HTTP method", default_value = "GET")]
    method: String,

    #[arg(short = 'H', long = "header", help = "Custom headers (can be used multiple times)")]
    headers: Vec<String>,

    #[arg(short = 'u', long = "user-agent", help = "Custom User-Agent")]
    user_agent: Option<String>,

    #[arg(short = 'q', long = "quiet", help = "Minimal output")]
    quiet: bool,

    #[arg(short = 'v', long = "verbose", help = "Verbose output with headers")]
    verbose: bool,

    #[arg(short = 's', long = "stats-only", help = "Show only final statistics")]
    stats_only: bool,

    #[arg(long = "no-color", help = "Disable colored output")]
    no_color: bool,

    #[arg(long = "json", help = "JSON output format")]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Monitor multiple targets from a config file
    Monitor {
        #[arg(short, long, help = "Path to configuration file")]
        config: String,
    },
    /// Generate example configuration file
    Init {
        #[arg(short, long, help = "Output path for config file", default_value = "httpping.yml")]
        output: String,
    },
    /// Single URL ping (same as legacy mode)
    Ping {
        #[arg(help = "URL to ping")]
        url: String,
        
        #[arg(short = 'c', long = "count")]
        count: Option<u64>,
        
        #[arg(short = 'i', long = "interval", default_value = "1.0")]
        interval: f64,
        
        #[arg(short = 't', long = "timeout", default_value = "10.0")]
        timeout: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PingResult {
    sequence: u64,
    url: String,
    status_code: Option<u16>,
    response_time: Duration,
    success: bool,
    error: Option<String>,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PingStatistics {
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    success_rate: f64,
    min_response_time: Duration,
    max_response_time: Duration,
    avg_response_time: Duration,
    total_time: Duration,
}

struct HttpPinger {
    client: Client,
    url: String,
    args: Args,
    stats: Arc<PingStatistics>,
    running: Arc<AtomicBool>,
    sequence: Arc<AtomicU64>,
}

impl HttpPinger {
    pub fn get_random_user_agent() -> &'static str {
        const USER_AGENTS: &[&str] = &[
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/121.0",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Safari/605.1.15",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/120.0",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
            "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:109.0) Gecko/20100101 Firefox/121.0",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0",
        ];
        
        let mut rng = rand::thread_rng();
        USER_AGENTS.choose(&mut rng).unwrap()
    }

    fn new(args: Args) -> Result<Self, Box<dyn std::error::Error>> {
        let client = Client::builder()
            .timeout(Duration::from_secs_f64(args.timeout))
            .build()?;

        Ok(Self {
            client,
            url: args.url.clone().unwrap_or_default(),
            args,
            stats: Arc::new(PingStatistics {
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
                success_rate: 0.0,
                min_response_time: Duration::from_secs(u64::MAX),
                max_response_time: Duration::from_secs(0),
                avg_response_time: Duration::from_secs(0),
                total_time: Duration::from_secs(0),
            }),
            running: Arc::new(AtomicBool::new(true)),
            sequence: Arc::new(AtomicU64::new(0)),
        })
    }

    async fn ping_once(&self) -> PingResult {
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let start = Instant::now();

        let method = match self.args.method.to_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            "HEAD" => Method::HEAD,
            "OPTIONS" => Method::OPTIONS,
            "PATCH" => Method::PATCH,
            _ => Method::GET,
        };

        let mut request_builder = self.client.request(method, &self.url);

        // Use custom User-Agent if provided, otherwise use random one
        let user_agent = self.args.user_agent.as_deref().unwrap_or_else(|| Self::get_random_user_agent());
        request_builder = request_builder.header("User-Agent", user_agent);

        for header in &self.args.headers {
            if let Some((key, value)) = header.split_once(':') {
                request_builder = request_builder.header(key.trim(), value.trim());
            }
        }

        match request_builder.send().await {
            Ok(response) => {
                let response_time = start.elapsed();
                let status_code = response.status();
                let success = status_code.is_success();

                PingResult {
                    sequence: seq,
                    url: self.url.clone(),
                    status_code: Some(status_code.as_u16()),
                    response_time,
                    success,
                    error: None,
                    timestamp: chrono::Utc::now(),
                }
            }
            Err(err) => {
                let response_time = start.elapsed();
                PingResult {
                    sequence: seq,
                    url: self.url.clone(),
                    status_code: None,
                    response_time,
                    success: false,
                    error: Some(err.to_string()),
                    timestamp: chrono::Utc::now(),
                }
            }
        }
    }

    fn update_stats(&mut self, result: &PingResult) {
        let stats = Arc::get_mut(&mut self.stats).unwrap();
        stats.total_requests += 1;

        if result.success {
            stats.successful_requests += 1;
        } else {
            stats.failed_requests += 1;
        }

        stats.success_rate = (stats.successful_requests as f64 / stats.total_requests as f64) * 100.0;

        if result.response_time < stats.min_response_time {
            stats.min_response_time = result.response_time;
        }
        if result.response_time > stats.max_response_time {
            stats.max_response_time = result.response_time;
        }

        let total_time_ms = (stats.avg_response_time.as_millis() as u64 * (stats.total_requests - 1)) + result.response_time.as_millis() as u64;
        stats.avg_response_time = Duration::from_millis(total_time_ms / stats.total_requests);
    }

    fn format_response_time(&self, duration: Duration) -> String {
        let ms = duration.as_millis();
        if self.args.no_color {
            format!("{}ms", ms)
        } else {
            match ms {
                0..=50 => format!("{}ms", ms).green().to_string(),
                51..=200 => format!("{}ms", ms).yellow().to_string(),
                _ => format!("{}ms", ms).red().to_string(),
            }
        }
    }

    fn format_status_code(&self, status_code: Option<u16>) -> String {
        match status_code {
            Some(code) => {
                if self.args.no_color {
                    format!("{}", code)
                } else {
                    match code {
                        200..=299 => format!("{}", code).green().to_string(),
                        300..=399 => format!("{}", code).yellow().to_string(),
                        400..=599 => format!("{}", code).red().to_string(),
                        _ => format!("{}", code).white().to_string(),
                    }
                }
            }
            None => {
                if self.args.no_color {
                    "TIMEOUT/ERROR".to_string()
                } else {
                    "TIMEOUT/ERROR".red().to_string()
                }
            }
        }
    }

    fn print_result(&self, result: &PingResult) {
        if self.args.json {
            println!("{}", serde_json::to_string(result).unwrap());
            return;
        }

        if self.args.stats_only {
            return;
        }

        let status_str = self.format_status_code(result.status_code);
        let time_str = self.format_response_time(result.response_time);
        let success_indicator = if result.success {
            if self.args.no_color { "✓".to_string() } else { "✓".green().to_string() }
        } else {
            if self.args.no_color { "✗".to_string() } else { "✗".red().to_string() }
        };

        if self.args.quiet {
            println!("{} {} {}", success_indicator, status_str, time_str);
        } else {
            println!("PING {} [{}]: seq={} status={} time={}",
                     self.url,
                     success_indicator,
                     result.sequence,
                     status_str,
                     time_str);

            if self.args.verbose {
                if let Some(error) = &result.error {
                    println!("  Error: {}", error);
                }
            }
        }
    }

    fn print_statistics(&self) {
        if self.args.json {
            println!("{}", serde_json::to_string(&*self.stats).unwrap());
            return;
        }

        println!();
        println!("--- {} ping statistics ---", self.url);
        println!("{} packets transmitted, {} received, {:.1}% packet loss",
                 self.stats.total_requests,
                 self.stats.successful_requests,
                 100.0 - self.stats.success_rate);

        if self.stats.successful_requests > 0 {
            println!("round-trip min/avg/max = {}/{}/{} ms",
                     self.stats.min_response_time.as_millis(),
                     self.stats.avg_response_time.as_millis(),
                     self.stats.max_response_time.as_millis());
        }
    }

    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let running = Arc::clone(&self.running);
        
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })?;

        let start_time = Instant::now();
        let mut count = 0u64;

        while self.running.load(Ordering::SeqCst) {
            if let Some(max_count) = self.args.count {
                if count >= max_count {
                    break;
                }
            }

            let result = self.ping_once().await;
            self.update_stats(&result);
            self.print_result(&result);

            count += 1;

            if self.running.load(Ordering::SeqCst) {
                sleep(Duration::from_secs_f64(self.args.interval)).await;
            }
        }

        let stats = Arc::get_mut(&mut self.stats).unwrap();
        stats.total_time = start_time.elapsed();

        self.print_statistics();
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.no_color {
        colored::control::set_override(false);
    }

    match args.command {
        Some(Commands::Monitor { config }) => {
            let config = Config::from_file(&config)?;
            let monitor = Monitor::new(config)?;
            monitor.run().await?;
        }
        Some(Commands::Init { output }) => {
            let example_config = Config::example();
            let yaml = serde_yaml::to_string(&example_config)?;
            std::fs::write(&output, yaml)?;
            println!("✅ Example configuration written to: {}", output);
            println!("📝 Edit the file and run: httpping monitor -c {}", output);
        }
        Some(Commands::Ping { url, count, interval, timeout }) => {
            // Convert to legacy args format
            let legacy_args = Args {
                command: None,
                url: Some(url),
                count,
                interval,
                timeout,
                method: "GET".to_string(),
                headers: vec![],
                user_agent: None,
                quiet: false,
                verbose: false,
                stats_only: false,
                no_color: args.no_color,
                json: args.json,
            };
            
            let mut pinger = HttpPinger::new(legacy_args)?;
            pinger.run().await?;
        }
        None => {
            // Legacy mode - direct URL argument
            if let Some(url) = args.url {
                let legacy_args = Args {
                    url: Some(url),
                    ..args
                };
                let mut pinger = HttpPinger::new(legacy_args)?;
                pinger.run().await?;
            } else {
                eprintln!("❌ Error: URL required or use a subcommand");
                eprintln!("Usage: httpping <URL> or httpping <COMMAND>");
                eprintln!("Try: httpping --help");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}