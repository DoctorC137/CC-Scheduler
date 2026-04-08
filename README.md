![Clever Cloud logo](github-assets/clever-cloud-logo.png)

# POC Scheduler CC
[![Clever Cloud - PaaS](https://img.shields.io/badge/Clever%20Cloud-PaaS-orange)](https://clever-cloud.com)
[![Rust](https://img.shields.io/badge/Rust-1.94-000000?logo=rust)](https://www.rust-lang.org)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-15-4169E1?logo=postgresql)](https://www.postgresql.org)

> **POC** — Self-hosted scheduler that automatically starts and stops Clever Cloud applications on a cron schedule. One deployment manages one organisation. Multiple instances run safely in parallel via a PostgreSQL advisory lock that guarantees each job fires exactly once.

---

## Architecture

| Service | Role |
|---|---|
| **Rust / Axum** | Web framework — REST API + embedded SPA |
| **tokio-cron-scheduler** | In-process cron engine with IANA timezone support |
| **PostgreSQL** | Schedule persistence + distributed advisory locking |
| **Clever Cloud API** | Start / stop applications via Biscuit service token |

---

## How it works

Each schedule targets one application and defines:
- An optional **stop** cron — sets `minInstances=0`, then deletes running instances
- An optional **start** cron — sets `minInstances=1`, then triggers a new deployment

Clever Cloud automatically restarts any app whose `minInstances ≥ 1`. Stop must write `0` first — otherwise the platform re-schedules a deployment the moment instances are deleted.

### Distributed locking

When multiple instances run simultaneously, each registers its own cron jobs in-memory. Before calling the Clever Cloud API, every job acquires a **PostgreSQL transaction-level advisory lock** (`pg_try_advisory_xact_lock`):

- **Acquired** → execute, write to `execution_logs`, commit (releases lock atomically)
- **Not acquired** → another instance is already handling this job; skip and return

Transaction-level locks are automatically released on commit or rollback, and are safe with connection pools — unlike session-level locks, they never leak.

---

## Repository structure

```
.
├── src/
│   ├── main.rs        Boot: connect DB, init scheduler, start Axum server
│   ├── config.rs      Load environment variables
│   ├── db.rs          PostgreSQL layer (SQLx, inline migrations)
│   ├── clever.rs      Clever Cloud API client (Bearer auth)
│   ├── scheduler.rs   Cron job registry with distributed locking
│   ├── api.rs         Axum routes + AppState
│   ├── auth.rs        Session cookie middleware (HMAC-SHA1)
│   ├── error.rs       Unified error type → HTTP responses
│   └── frontend.html  Embedded single-page UI
└── github-assets/
    └── clever-cloud-logo.png
```

---

## Deployment

### Prerequisites

```bash
npm install -g clever-tools
clever login
```

### 1. Create the app and add-on

```bash
clever create --type rust cc-scheduler --region par --org <org_id>
clever addon create postgresql-addon --plan dev --link cc-scheduler
```

### 2. Create a service token for the organisation

```bash
curl -X POST https://api.clever-cloud.com/v2/organisations/<org_id>/service-tokens \
  -H "Content-Type: application/json" \
  -d '{"name":"cc-scheduler","role":"MANAGER","expirationDate":"2027-12-31T00:00:00Z"}'
```

> The token requires the `MANAGER` role to read scalability settings and start/stop applications.

### 3. Set environment variables

```bash
clever env set CC_ORG_ID        "orga_xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
clever env set CC_SERVICE_TOKEN "<biscuit_token>"
clever env set APP_PASSWORD     "<web_ui_password>"
```

| Variable               | Description                              | Source         |
|------------------------|------------------------------------------|----------------|
| `PORT`                 | HTTP port (default: `8080`)              | Injected by CC |
| `POSTGRESQL_ADDON_URI` | PostgreSQL connection string             | Injected by CC |
| `CC_ORG_ID`            | ID of the organisation to manage         | Set manually   |
| `CC_SERVICE_TOKEN`     | Biscuit service token (MANAGER role)     | Set manually   |
| `APP_PASSWORD`         | Password for the web interface           | Set manually   |
| `RUST_LOG`             | Log level (e.g. `info`, `debug`)         | Optional       |

### 4. Deploy

```bash
clever deploy
```

---

## Local development

```bash
# Start a local PostgreSQL instance
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=dev postgres:16

# Run the app
DATABASE_URL=postgres://postgres:dev@localhost/cc_scheduler \
CC_ORG_ID=orga_xxx \
CC_SERVICE_TOKEN=<biscuit> \
APP_PASSWORD=secret \
cargo run
```

The UI is available at [http://localhost:8080](http://localhost:8080).

---

## REST API

All routes require a valid session cookie (obtained via `POST /auth/login`).

### Schedules

```
GET    /schedules                     List all schedules
POST   /schedules                     Create a schedule
GET    /schedules/:id                 Get a schedule
PUT    /schedules/:id                 Update a schedule
DELETE /schedules/:id                 Delete a schedule
POST   /schedules/:id/trigger/start   Start the app immediately
POST   /schedules/:id/trigger/stop    Stop the app immediately
```

**Example — create a schedule:**

```json
POST /schedules
{
  "org_id":     "orga_xxx",
  "app_id":     "app_xxx",
  "name":       "Staging — weekdays only",
  "cron_stop":  "0 0 20 * * 1-5",
  "cron_start": "0 0 8 * * 1-5",
  "timezone":   "Europe/Paris",
  "enabled":    true
}
```

`cron_stop` and `cron_start` are independent — a schedule can define either or both.

### Organisation

```
GET /orgs              Returns the configured organisation
GET /orgs/:id/apps     Lists all applications in the organisation
```

---

## Cron format

Expressions use the **6-field** format required by `tokio-cron-scheduler`:

```
sec  min  hour  day-of-month  month  day-of-week
 0    0    20        *           *       1-5
```

| Expression          | Meaning                         |
|---------------------|---------------------------------|
| `0 0 20 * * 1-5`   | 8:00 PM, Monday–Friday          |
| `0 0 8 * * 1-5`    | 8:00 AM, Monday–Friday          |
| `0 0 22 * * *`     | 10:00 PM every day              |
| `0 30 7 1 * *`     | 7:30 AM on the 1st of the month |
| `0 0 0 * * 6,0`    | Midnight on weekends            |

Timezones are IANA strings (e.g. `Europe/Paris`, `UTC`, `America/New_York`). The scheduler applies the timezone — cron times are **local, not UTC**.

---

## Testing

```bash
# Unit tests — mock HTTP server, no credentials needed
cargo test

# Integration tests — against the real Clever Cloud API
cargo test -- --ignored --test-threads=1
```

Integration tests require `CC_ORG_ID` and `CC_SERVICE_TOKEN` in the environment or a `.env` file.

---

## Security

- **Web UI** — password-protected; session is an HMAC-SHA1 cookie (HttpOnly, SameSite=Lax, 7-day TTL)
- **Clever Cloud API** — Biscuit service token scoped to one organisation; revocable from the CC console at any time
- **Isolation** — one deployment = one organisation = one dedicated token

---

## Additional resources

- [Clever Cloud Documentation](https://www.clever-cloud.com/doc/)
- [Clever Tools CLI](https://github.com/CleverCloud/clever-tools)
- [Clever Cloud Status](https://status.clever-cloud.com/)
