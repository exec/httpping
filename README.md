# 🌐 httpping

**Advanced HTTP monitoring and ping utility for modern infrastructure**

httpping is a powerful command-line tool that goes beyond simple HTTP pings to provide comprehensive web service monitoring. Think of it as "ping for HTTP" with intelligence, alerting, and production-ready monitoring features.

## ✨ Features

### 🎯 Single URL Ping
- **Fast HTTP/HTTPS ping** with detailed timing
- **Smart User-Agent rotation** to bypass bot protection
- **Colored output** with response time indicators
- **Response code validation** with visual status
- **JSON output** for automation and scripting

### 🚀 Multi-Target Monitoring
- **Concurrent monitoring** of multiple endpoints
- **YAML configuration** for complex monitoring setups
- **Health scoring** with configurable thresholds
- **Real-time status dashboard** in terminal
- **Uptime percentage** and SLA tracking

### 🔔 Intelligent Alerting
- **Slack/Discord webhooks** for instant notifications
- **Smart alert cooldowns** to prevent spam
- **Multiple trigger conditions** (response time, failures, cert expiry)
- **Certificate expiration monitoring** for HTTPS sites

### 📊 Production Ready
- **Response time analytics** (min/avg/max)
- **Success rate tracking** over time windows
- **Expected content validation** beyond status codes
- **Custom headers** and HTTP methods
- **Graceful shutdown** with comprehensive summaries

## 🚀 Quick Start

### Install
```bash
cargo install --git https://github.com/username/httpping
```

### Simple Usage
```bash
# Basic ping
httpping https://example.com

# Limited count with custom interval
httpping https://api.example.com -c 10 -i 2.0

# Quiet mode for scripting
httpping https://service.com -q -c 5

# JSON output for automation
httpping https://api.com --json -c 3
```

### Advanced Monitoring
```bash
# Generate example config
httpping init

# Edit httpping.yml, then run:
httpping monitor -c httpping.yml
```

## 📋 Configuration

Generate an example configuration file:

```bash
httpping init --output my-monitors.yml
```

Example configuration:

```yaml
targets:
  - name: "Production API"
    url: "https://api.yoursite.com/health"
    method: GET
    expected_status: [200]
    expected_content: '"status":"ok"'
    timeout_seconds: 5.0
    interval_seconds: 30.0

  - name: "Main Website"  
    url: "https://yoursite.com"
    expected_status: [200, 301, 302]
    interval_seconds: 60.0

settings:
  max_consecutive_failures: 3
  health_check_window_minutes: 60
  enable_colors: true

alerts:
  - name: "Slack Production Alerts"
    webhook_url: "https://hooks.slack.com/services/YOUR/WEBHOOK/URL"
    trigger_on:
      - !consecutive_failures 3
      - !response_time_ms 5000  
      - !cert_expiring_days 7
    cooldown_minutes: 30
```

## 🎨 Output Examples

### Single URL Ping
```
PING https://api.example.com [✓]: seq=1 status=200 time=145ms
PING https://api.example.com [✓]: seq=2 status=200 time=132ms
PING https://api.example.com [✗]: seq=3 status=500 time=167ms

--- https://api.example.com ping statistics ---
3 packets transmitted, 2 received, 33.3% packet loss
round-trip min/avg/max = 132/148/167 ms
```

### Multi-Target Dashboard
```
🚀 Starting HTTP monitor for 3 targets...

[14:32:15] ✓ Production API | 200 | 145ms
[14:32:18] ✓ Main Website | 200 | 267ms  
[14:32:20] ✗ Staging API | 503 | 89ms
    Error: Service temporarily unavailable

📊 Status Summary:
Target               Status     Uptime      Avg Response   Health
──────────────────────────────────────────────────────────────────
Production API       Healthy    99.8%       156ms          98.2
Main Website         Healthy    100.0%      245ms          95.8
Staging API          Unhealthy  87.2%       198ms          23.1
```

## 🔧 Command Reference

### Single URL Commands
```bash
httpping <URL> [OPTIONS]
httpping ping <URL> [OPTIONS]
```

Options:
- `-c, --count <NUM>` - Number of requests (default: infinite)
- `-i, --interval <SEC>` - Interval between requests (default: 1.0)
- `-t, --timeout <SEC>` - Request timeout (default: 10.0) 
- `-m, --method <METHOD>` - HTTP method (default: GET)
- `-H, --header <HEADER>` - Custom headers (repeatable)
- `-u, --user-agent <UA>` - Custom User-Agent
- `-q, --quiet` - Minimal output
- `--json` - JSON output format
- `--no-color` - Disable colors

### Multi-Target Commands
```bash
httpping init [--output CONFIG]     # Generate example config
httpping monitor -c <CONFIG>        # Run monitoring from config
```

## 🤔 Why httpping?

**vs. curl**: httpping provides continuous monitoring, statistics, and alerting - not just one-off requests

**vs. ping**: httpping works over HTTP/HTTPS with content validation, status codes, and response times

**vs. Nagios/DataDog**: httpping is lightweight, easy to deploy, and perfect for startups or side projects needing basic monitoring

**vs. simple scripts**: httpping handles User-Agent rotation, smart alerting, graceful shutdown, and production-ready error handling

## 📊 Use Cases

- **API health monitoring** - Ensure your services stay online
- **Website uptime tracking** - Monitor main sites and landing pages  
- **CI/CD integration** - Verify deployments with automated checks
- **Load testing prep** - Baseline response times before scaling
- **Certificate monitoring** - Get warned before HTTPS certs expire
- **SLA compliance** - Track uptime percentages for service agreements

## 🛠️ Development

```bash
git clone https://github.com/username/httpping
cd httpping
cargo build --release
./target/release/httpping --help
```

## 📜 License

MIT License - see LICENSE file for details.

## 🤝 Contributing

Issues and pull requests welcome! This tool was built to solve real infrastructure monitoring needs.