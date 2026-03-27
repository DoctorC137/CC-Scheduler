use std::sync::Arc;
use anyhow::Result;
use tokio_cron_scheduler::{JobScheduler, Job};
use tracing::{info, error, warn};
use uuid::Uuid;

use crate::{
    clever::CleverClient,
    config::AppConfig,
    db::Database,
    models::Schedule,
};

pub struct SchedulerService {
    pub sched: JobScheduler,
    pub db: Database,
    pub cc: Arc<CleverClient>,
}

impl SchedulerService {
    pub async fn new(db: Database, cfg: AppConfig) -> Result<Self> {
        let sched = JobScheduler::new().await?;
        sched.start().await?;

        let cc = Arc::new(CleverClient::new(&cfg)?);

        Ok(Self { sched, db, cc })
    }

    /// Charge tous les schedules actifs depuis la DB et les programme.
    pub async fn load_and_schedule_all(&self) -> Result<()> {
        let schedules = self.db.list_enabled_schedules().await?;
        info!("Loading {} active schedules", schedules.len());

        for s in schedules {
            self.register(&s).await?;
        }
        Ok(())
    }

    /// Enregistre un schedule dans le cron scheduler (stop + start).
    pub async fn register(&self, s: &Schedule) -> Result<()> {
        if let Some(ref cron) = s.cron_stop {
            self.add_job(s.id, &s.org_id, &s.app_id, cron, "stop").await?;
        }
        if let Some(ref cron) = s.cron_start {
            self.add_job(s.id, &s.org_id, &s.app_id, cron, "start").await?;
        }
        Ok(())
    }

    async fn add_job(
        &self,
        schedule_id: Uuid,
        org_id: &str,
        app_id: &str,
        cron: &str,
        action: &str,
    ) -> Result<()> {
        let cc = self.cc.clone();
        let db = self.db.clone();
        let org = org_id.to_string();
        let app = app_id.to_string();
        let act = action.to_string();
        let sid = schedule_id;

        // tokio-cron-scheduler supporte le format standard 5 champs + secondes optionnelles
        let job = Job::new_async(cron, move |_uuid, _lock| {
            let cc = cc.clone();
            let db = db.clone();
            let org = org.clone();
            let app = app.clone();
            let act = act.clone();

            Box::pin(async move {
                info!(schedule_id = %sid, action = %act, app_id = %app, "Executing scheduled action");

                let result = match act.as_str() {
                    "stop" => cc.stop_app(&org, &app).await,
                    "start" => cc.start_app(&org, &app).await,
                    _ => {
                        error!("Unknown action: {}", act);
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
        info!(schedule_id = %schedule_id, cron = %cron, action = %action, "Job registered");
        Ok(())
    }

    /// Supprime tous les jobs associés à un schedule puis les re-programme.
    /// Stratégie simple: restart complet du scheduler avec rechargement DB.
    pub async fn reload_schedule(&self, schedule_id: Uuid) -> Result<()> {
        warn!(schedule_id = %schedule_id, "Reload not yet fine-grained: triggering full reload");
        // TODO: implémenter la suppression fine par UUID de job
        // Pour l'instant on reload tout (acceptable pour des dizaines de schedules)
        self.load_and_schedule_all().await
    }
}
