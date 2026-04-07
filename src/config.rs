use anyhow::Result;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub port: u16,
    pub database_url: String,
    pub cc_org_id: String,
    pub cc_service_token: String,
    pub app_password: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()?,
            database_url: std::env::var("POSTGRESQL_ADDON_URI")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .expect("POSTGRESQL_ADDON_URI or DATABASE_URL must be set"),
            cc_org_id: std::env::var("CC_ORG_ID")
                .expect("CC_ORG_ID must be set"),
            cc_service_token: std::env::var("CC_SERVICE_TOKEN")
                .expect("CC_SERVICE_TOKEN must be set"),
            app_password: std::env::var("APP_PASSWORD")
                .expect("APP_PASSWORD must be set"),
        })
    }
}
