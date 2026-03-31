use anyhow::Result;
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
}

impl SchedulerService {
    pub async fn new(db: Database) -> Result<Self> {
        let sched = JobScheduler::new().await?;
        sched.start().await?;
        Ok(Self { sched, db })
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
        if s.user_id.is_none() {
            warn!(schedule_id = %s.id, "Schedule sans user_id, ignoré");
            return Ok(());
        }
        if let Some(ref cron) = s.cron_stop {
            self.add_job(s, cron, "stop").await?;
        }
        if let Some(ref cron) = s.cron_start {
            self.add_job(s, cron, "start").await?;
        }
        Ok(())
    }

    async fn add_job(&self, s: &Schedule, cron: &str, action: &str) -> Result<()> {
        let db = self.db.clone();
        let user_id = s.user_id.unwrap();
        let org = s.org_id.clone();
        let app = s.app_id.clone();
        let act = action.to_string();
        let sid = s.id;

        let job = Job::new_async(cron, move |_uuid, _lock| {
            let db = db.clone();
            let org = org.clone();
            let app = app.clone();
            let act = act.clone();

            Box::pin(async move {
                info!(schedule_id = %sid, action = %act, app_id = %app, "Executing scheduled action");

                // Récupère le token de l'utilisateur propriétaire du schedule
                let user = match db.get_user(user_id).await {
                    Ok(u) => u,
                    Err(e) => {
                        error!(schedule_id = %sid, "Failed to get user token: {}", e);
                        let _ = db.record_execution(sid, &act, Some(&e.to_string())).await;
                        return;
                    }
                };

                let cc = CleverClient::new(user.access_token, user.access_secret);
                let result = match act.as_str() {
                    "stop" => cc.stop_app(&org, &app).await,
                    "start" => cc.start_app(&org, &app).await,
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
        // TODO: suppression fine par job UUID
        // Pour l'instant, rechargement complet acceptable pour des dizaines de schedules
        self.load_and_schedule_all().await
    }
}
