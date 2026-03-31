use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Un schedule d'extinction/allumage pour une application Clever Cloud.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Schedule {
    pub id: Uuid,

    /// ID de l'organisation CC (orga_xxxx)
    pub org_id: String,

    /// ID de l'application CC (app_xxxx)
    pub app_id: String,

    /// Nom lisible (facultatif)
    pub name: Option<String>,

    /// Expression cron pour l'extinction (ex: "0 20 * * 1-5")
    /// None = pas d'extinction programmée
    pub cron_stop: Option<String>,

    /// Expression cron pour le démarrage (ex: "0 8 * * 1-5")
    /// None = pas de démarrage programmé
    pub cron_start: Option<String>,

    /// Fuseau horaire (ex: "Europe/Paris")
    pub timezone: String,

    /// Le schedule est-il actif ?
    pub enabled: bool,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_action: Option<String>,  // "stop" | "start"
    pub last_error: Option<String>,
}

/// Payload pour créer un schedule
#[derive(Debug, Deserialize)]
pub struct CreateSchedule {
    pub org_id: String,
    pub app_id: String,
    pub name: Option<String>,
    pub cron_stop: Option<String>,
    pub cron_start: Option<String>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Payload pour modifier un schedule
#[derive(Debug, Deserialize)]
pub struct UpdateSchedule {
    pub name: Option<String>,
    pub cron_stop: Option<Option<String>>,
    pub cron_start: Option<Option<String>>,
    pub timezone: Option<String>,
    pub enabled: Option<bool>,
}

fn default_timezone() -> String {
    "Europe/Paris".to_string()
}

fn default_true() -> bool {
    true
}
