use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    clever::CleverClient,
    db::Database,
    models::Schedule,
};

pub struct SchedulerService {
    pub sched: JobScheduler,
    pub db: Database,
    pub cc: Arc<CleverClient>,
    /// Maps schedule_id → list of cron job UUIDs registered for it.
    /// Used to remove stale jobs before re-registering on update.
    job_map: Arc<Mutex<HashMap<Uuid, Vec<uuid::Uuid>>>>,
}

/// Derives a stable 64-bit advisory lock key from a schedule ID + action.
///
/// Uses XOR of the two UUID halves so all 128 bits of entropy are folded in.
/// The LSB encodes the action (0 = stop, 1 = start) to guarantee that a
/// stop and a start on the same schedule never compete for the same lock.
fn advisory_lock_key(schedule_id: Uuid, action: &str) -> i64 {
    let b = schedule_id.as_bytes();
    let base = i64::from_be_bytes(b[0..8].try_into().unwrap())
        ^ i64::from_be_bytes(b[8..16].try_into().unwrap());
    let action_bit = if action == "start" { 1i64 } else { 0i64 };
    (base & !1i64) | action_bit
}

impl SchedulerService {
    pub async fn new(db: Database, cc: Arc<CleverClient>) -> Result<Self> {
        let sched = JobScheduler::new().await?;
        sched.start().await?;
        Ok(Self {
            sched,
            db,
            cc,
            job_map: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn load_and_schedule_all(&self) -> Result<()> {
        let schedules = self.db.list_enabled_schedules().await?;
        info!("Loading {} active schedules", schedules.len());
        for s in schedules {
            if let Err(e) = self.register(&s).await {
                warn!(schedule_id = %s.id, "Failed to register schedule: {}", e);
            }
        }
        Ok(())
    }

    pub async fn register(&self, s: &Schedule) -> Result<()> {
        if let Some(ref cron) = s.cron_stop {
            self.add_job(s, cron, "stop").await?;
        }
        if let Some(ref cron) = s.cron_start {
            self.add_job(s, cron, "start").await?;
        }
        Ok(())
    }

    async fn add_job(&self, s: &Schedule, cron: &str, action: &str) -> Result<()> {
        let cc = self.cc.clone();
        let db = self.db.clone();
        let app = s.app_id.clone();
        let act = action.to_string();
        let sid = s.id;

        let tz: chrono_tz::Tz = s.timezone.parse().unwrap_or_else(|_| {
            warn!(schedule_id = %sid, timezone = %s.timezone, "Invalid timezone, falling back to Europe/Paris");
            chrono_tz::Europe::Paris
        });

        // Pre-compute the advisory lock key (stable, deterministic).
        let lock_key = advisory_lock_key(sid, action);

        let job = Job::new_async_tz(cron, tz, move |_uuid, _lock| {
            let cc = cc.clone();
            let db = db.clone();
            let app = app.clone();
            let act = act.clone();

            Box::pin(async move {
                info!(schedule_id = %sid, action = %act, app_id = %app, "Attempting scheduled action");

                // ── Distributed lock ──────────────────────────────────────────
                // Open a transaction and try to acquire a PostgreSQL
                // transaction-level advisory lock.  Only one instance can hold
                // a given key at a time.  The lock is automatically released
                // when the transaction commits or rolls back, making it safe
                // for use with a connection pool (unlike session-level locks).
                let mut tx = match db.pool().begin().await {
                    Ok(tx) => tx,
                    Err(e) => {
                        error!(schedule_id = %sid, "Failed to open transaction: {}", e);
                        return;
                    }
                };

                let acquired: bool = sqlx::query_scalar(
                    "SELECT pg_try_advisory_xact_lock($1)"
                )
                .bind(lock_key)
                .fetch_one(&mut *tx)
                .await
                .unwrap_or(false);

                if !acquired {
                    info!(
                        schedule_id = %sid, action = %act,
                        "Skipping: another instance is already handling this job"
                    );
                    return; // tx is dropped here → lock never held, no-op rollback
                }

                info!(schedule_id = %sid, action = %act, app_id = %app, "Lock acquired, executing");

                // ── Execute ───────────────────────────────────────────────────
                let result = match act.as_str() {
                    "stop"  => cc.stop_app(&app).await,
                    "start" => cc.start_app(&app).await,
                    _ => {
                        error!(schedule_id = %sid, "Unknown action: {}", act);
                        return;
                    }
                };

                let error_str: Option<String> = match &result {
                    Ok(_) => {
                        info!(schedule_id = %sid, action = %act, "Action succeeded");
                        None
                    }
                    Err(e) => {
                        error!(schedule_id = %sid, action = %act, error = %e, "Action failed");
                        Some(e.to_string())
                    }
                };

                // ── Record inside the same transaction ────────────────────────
                // Committing both releases the advisory lock and persists the
                // execution record atomically.
                let _ = sqlx::query(
                    "INSERT INTO execution_logs (schedule_id, action, error) \
                     VALUES ($1, $2, $3)"
                )
                .bind(sid)
                .bind(act.as_str())
                .bind(error_str.as_deref())
                .execute(&mut *tx)
                .await;

                let _ = sqlx::query(
                    "UPDATE schedules \
                     SET last_run_at = now(), last_action = $2, last_error = $3 \
                     WHERE id = $1"
                )
                .bind(sid)
                .bind(act.as_str())
                .bind(error_str.as_deref())
                .execute(&mut *tx)
                .await;

                if let Err(e) = tx.commit().await {
                    error!(schedule_id = %sid, "Failed to commit after action: {}", e);
                }
            })
        })?;

        let job_uuid = self.sched.add(job).await?;
        self.job_map.lock().await.entry(sid).or_default().push(job_uuid);
        info!(schedule_id = %sid, cron = %cron, action = %action, "Job registered");
        Ok(())
    }

    /// Removes the old cron jobs for this schedule, then re-registers if still enabled.
    pub async fn reload_schedule(&self, schedule_id: Uuid) -> Result<()> {
        let old_jobs = self.job_map.lock().await.remove(&schedule_id).unwrap_or_default();
        for job_id in old_jobs {
            if let Err(e) = self.sched.remove(&job_id).await {
                warn!(schedule_id = %schedule_id, "Failed to remove old job {}: {}", job_id, e);
            }
        }

        match self.db.get_schedule(schedule_id).await {
            Ok(schedule) if schedule.enabled => {
                self.register(&schedule).await?;
            }
            Ok(_) => {
                info!(schedule_id = %schedule_id, "Schedule disabled, not re-registering");
            }
            Err(e) => {
                warn!(schedule_id = %schedule_id, "Schedule not found after reload: {}", e);
            }
        }
        Ok(())
    }
}
