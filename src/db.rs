use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

use crate::models::{CreateSchedule, Schedule, Session, UpdateSchedule, User};

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
        // Users : compte CC de chaque personne authentifiée
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS users (
                id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                cc_user_id    TEXT NOT NULL UNIQUE,
                cc_email      TEXT,
                access_token  TEXT NOT NULL,
                access_secret TEXT NOT NULL,
                created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#)
        .execute(&self.pool)
        .await?;

        // Sessions HTTP
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + INTERVAL '7 days'
            )
        "#)
        .execute(&self.pool)
        .await?;

        // Tokens de request OAuth1 temporaires (pendant le handshake)
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS oauth_requests (
                request_token  TEXT PRIMARY KEY,
                request_secret TEXT NOT NULL,
                created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        "#)
        .execute(&self.pool)
        .await?;

        // Schedules
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS schedules (
                id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id     UUID REFERENCES users(id) ON DELETE CASCADE,
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

        // Migration douce : ajouter user_id si la table existait sans elle
        sqlx::query(r#"
            ALTER TABLE schedules ADD COLUMN IF NOT EXISTS
                user_id UUID REFERENCES users(id) ON DELETE CASCADE
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

    // ── Users ────────────────────────────────────────────────────────────────

    /// Crée ou met à jour le compte user (upsert sur cc_user_id).
    pub async fn upsert_user(
        &self,
        cc_user_id: &str,
        cc_email: Option<&str>,
        access_token: &str,
        access_secret: &str,
    ) -> Result<User> {
        let row = sqlx::query_as::<_, User>(r#"
            INSERT INTO users (cc_user_id, cc_email, access_token, access_secret)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (cc_user_id) DO UPDATE SET
                cc_email      = EXCLUDED.cc_email,
                access_token  = EXCLUDED.access_token,
                access_secret = EXCLUDED.access_secret,
                updated_at    = now()
            RETURNING *
        "#)
        .bind(cc_user_id)
        .bind(cc_email)
        .bind(access_token)
        .bind(access_secret)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn get_user(&self, user_id: Uuid) -> Result<User> {
        let row = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    // ── Sessions ─────────────────────────────────────────────────────────────

    pub async fn create_session(&self, user_id: Uuid) -> Result<Session> {
        let row = sqlx::query_as::<_, Session>(r#"
            INSERT INTO sessions (user_id)
            VALUES ($1)
            RETURNING *
        "#)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Retourne le User associé à cette session si elle existe et n'a pas expiré.
    pub async fn get_session_user(&self, session_id: Uuid) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, User>(r#"
            SELECT u.* FROM users u
            JOIN sessions s ON s.user_id = u.id
            WHERE s.id = $1 AND s.expires_at > now()
        "#)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_session(&self, session_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── OAuth1 request tokens (temporaires) ──────────────────────────────────

    pub async fn save_oauth_request(&self, token: &str, secret: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO oauth_requests (request_token, request_secret) VALUES ($1, $2)"
        )
        .bind(token)
        .bind(secret)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Récupère ET supprime le secret associé à un request token (one-shot).
    pub async fn get_and_delete_oauth_request(&self, token: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "DELETE FROM oauth_requests WHERE request_token = $1 RETURNING request_secret"
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(s,)| s))
    }

    // ── Schedules ────────────────────────────────────────────────────────────

    pub async fn list_schedules(&self, user_id: Uuid) -> Result<Vec<Schedule>> {
        let rows = sqlx::query_as::<_, Schedule>(
            "SELECT * FROM schedules WHERE user_id = $1 ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Tous les schedules actifs (pour le scheduler au démarrage — toutes users).
    pub async fn list_enabled_schedules(&self) -> Result<Vec<Schedule>> {
        let rows = sqlx::query_as::<_, Schedule>(
            "SELECT * FROM schedules WHERE enabled = TRUE AND user_id IS NOT NULL ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Récupère un schedule en vérifiant l'appartenance au user.
    pub async fn get_schedule(&self, id: Uuid, user_id: Uuid) -> Result<Schedule> {
        let row = sqlx::query_as::<_, Schedule>(
            "SELECT * FROM schedules WHERE id = $1 AND user_id = $2"
        )
        .bind(id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn create_schedule(&self, user_id: Uuid, p: CreateSchedule) -> Result<Schedule> {
        let row = sqlx::query_as::<_, Schedule>(r#"
            INSERT INTO schedules (user_id, org_id, app_id, name, cron_stop, cron_start, timezone, enabled)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
        "#)
        .bind(user_id)
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

    pub async fn update_schedule(&self, id: Uuid, user_id: Uuid, p: UpdateSchedule) -> Result<Schedule> {
        let row = sqlx::query_as::<_, Schedule>(r#"
            UPDATE schedules SET
                name       = COALESCE($3, name),
                cron_stop  = CASE WHEN $4 IS NOT NULL THEN $4 ELSE cron_stop END,
                cron_start = CASE WHEN $5 IS NOT NULL THEN $5 ELSE cron_start END,
                timezone   = COALESCE($6, timezone),
                enabled    = COALESCE($7, enabled),
                updated_at = now()
            WHERE id = $1 AND user_id = $2
            RETURNING *
        "#)
        .bind(id)
        .bind(user_id)
        .bind(p.name)
        .bind(p.cron_stop.flatten())
        .bind(p.cron_start.flatten())
        .bind(p.timezone)
        .bind(p.enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_schedule(&self, id: Uuid, user_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn record_execution(
        &self,
        schedule_id: Uuid,
        action: &str,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO execution_logs (schedule_id, action, error) VALUES ($1, $2, $3)"
        )
        .bind(schedule_id)
        .bind(action)
        .bind(error)
        .execute(&self.pool)
        .await?;

        sqlx::query(r#"
            UPDATE schedules SET
                last_run_at = now(),
                last_action = $2,
                last_error  = $3
            WHERE id = $1
        "#)
        .bind(schedule_id)
        .bind(action)
        .bind(error)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
