use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

use crate::models::{CreateSchedule, Schedule, UpdateSchedule};

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS schedules (
                id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                org_id      TEXT NOT NULL,
                app_id      TEXT NOT NULL,
                name        TEXT,
                cron_stop   TEXT,
                cron_start  TEXT,
                timezone    TEXT NOT NULL DEFAULT 'Europe/Paris',
                enabled     BOOLEAN NOT NULL DEFAULT TRUE,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
                last_run_at TIMESTAMPTZ,
                last_action TEXT,
                last_error  TEXT
            )
        "#)
        .execute(&self.pool)
        .await?;

        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS execution_logs (
                id          BIGSERIAL PRIMARY KEY,
                schedule_id UUID NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
                action      TEXT NOT NULL,
                executed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                error       TEXT
            )
        "#)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn list_schedules(&self) -> Result<Vec<Schedule>> {
        let rows = sqlx::query_as::<_, Schedule>(
            "SELECT id, org_id, app_id, name, cron_stop, cron_start, timezone, enabled, \
             created_at, updated_at, last_run_at, last_action, last_error \
             FROM schedules ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_enabled_schedules(&self) -> Result<Vec<Schedule>> {
        let rows = sqlx::query_as::<_, Schedule>(
            "SELECT id, org_id, app_id, name, cron_stop, cron_start, timezone, enabled, \
             created_at, updated_at, last_run_at, last_action, last_error \
             FROM schedules WHERE enabled = TRUE ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_schedule(&self, id: Uuid) -> Result<Schedule> {
        let row = sqlx::query_as::<_, Schedule>(
            "SELECT id, org_id, app_id, name, cron_stop, cron_start, timezone, enabled, \
             created_at, updated_at, last_run_at, last_action, last_error \
             FROM schedules WHERE id = $1"
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn create_schedule(&self, p: CreateSchedule) -> Result<Schedule> {
        let row = sqlx::query_as::<_, Schedule>(r#"
            INSERT INTO schedules (org_id, app_id, name, cron_stop, cron_start, timezone, enabled)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, org_id, app_id, name, cron_stop, cron_start, timezone, enabled,
                      created_at, updated_at, last_run_at, last_action, last_error
        "#)
        .bind(&p.org_id)
        .bind(&p.app_id)
        .bind(&p.name)
        .bind(&p.cron_stop)
        .bind(&p.cron_start)
        .bind(&p.timezone)
        .bind(p.enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn update_schedule(&self, id: Uuid, p: UpdateSchedule) -> Result<Schedule> {
        let row = sqlx::query_as::<_, Schedule>(r#"
            UPDATE schedules SET
                name       = COALESCE($2, name),
                cron_stop  = CASE WHEN $3 IS NOT NULL THEN $3 ELSE cron_stop END,
                cron_start = CASE WHEN $4 IS NOT NULL THEN $4 ELSE cron_start END,
                timezone   = COALESCE($5, timezone),
                enabled    = COALESCE($6, enabled),
                updated_at = now()
            WHERE id = $1
            RETURNING id, org_id, app_id, name, cron_stop, cron_start, timezone, enabled,
                      created_at, updated_at, last_run_at, last_action, last_error
        "#)
        .bind(id)
        .bind(p.name)
        .bind(p.cron_stop.flatten())
        .bind(p.cron_start.flatten())
        .bind(p.timezone)
        .bind(p.enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_schedule(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
