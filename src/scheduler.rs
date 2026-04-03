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

        let job = Job::new_async_tz(cron, tz, move |_uuid, _lock| {
            let cc = cc.clone();
            let db = db.clone();
            let app = app.clone();
            let act = act.clone();

            Box::pin(async move {
                info!(schedule_id = %sid, action = %act, app_id = %app, "Executing scheduled action");

                let result = match act.as_str() {
                    "stop" => cc.stop_app(&app).await,
                    "start" => cc.start_app(&app).await,
                    _ => {
                        error!(schedule_id = %sid, "Unknown action: {}", act);
                        return;
                    }
                };

                match result {
                    Ok(_) => {
                        info!(schedule_id = %sid, action = %act, "Action succeeded");
                        let _ = db.record_execution(sid, &act, None).await;
                    }
                    Err(e) => {
                        error!(schedule_id = %sid, action = %act, error = %e, "Action failed");
                        let _ = db.record_execution(sid, &act, Some(&e.to_string())).await;
                    }
                }
            })
        })?;

        let job_uuid = self.sched.add(job).await?;
        self.job_map.lock().await.entry(sid).or_default().push(job_uuid);
        info!(schedule_id = %sid, cron = %cron, action = %action, "Job registered");
        Ok(())
    }

    /// Removes the old cron jobs for this schedule, then re-registers if still enabled.
    /// Fixes the duplication bug where the previous implementation would accumulate
    /// duplicate jobs on every edit.
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
