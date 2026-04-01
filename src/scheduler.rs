use anyhow::Result;
use std::sync::Arc;
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
}

impl SchedulerService {
    pub async fn new(db: Database, cc: Arc<CleverClient>) -> Result<Self> {
        let sched = JobScheduler::new().await?;
        sched.start().await?;
        Ok(Self { sched, db, cc })
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

        let job = Job::new_async(cron, move |_uuid, _lock| {
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

        self.sched.add(job).await?;
        info!(schedule_id = %sid, cron = %cron, action = %action, "Job registered");
        Ok(())
    }

    pub async fn reload_schedule(&self, _schedule_id: Uuid) -> Result<()> {
        self.load_and_schedule_all().await
    }
}
