# cc-scheduler

Scheduler d'extinction/allumage automatique d'applications Clever Cloud, pour une organisation.

## Architecture

- **Rust** + **Axum** (API REST)
- **tokio-cron-scheduler** (exécution des tâches cron)
- **PostgreSQL** (add-on Clever Cloud, persistance des schedules)
- **clevercloud-sdk** (interactions avec l'API CC)

## Prérequis

1. Un compte Clever Cloud avec une organisation
2. Un **API Token CC** : console → profil → Tokens → créer un token
3. Un **add-on PostgreSQL** attaché à l'application

## Variables d'environnement

| Variable               | Description                                      | Injectée par CC |
|------------------------|--------------------------------------------------|-----------------|
| `PORT`                 | Port HTTP (8080 par défaut)                      | ✓               |
| `POSTGRESQL_ADDON_URI` | URL de connexion PostgreSQL                      | ✓ (add-on)      |
| `CC_API_TOKEN`         | Token API Clever Cloud (lecture + actions apps)  | À configurer    |
| `RUST_LOG`             | Niveau de log (ex: `info,cc_scheduler=debug`)    | À configurer    |

## Déploiement sur Clever Cloud

```bash
# 1. Créer l'application
clever create --type rust cc-scheduler --region par --org <org_id>

# 2. Ajouter le PostgreSQL
clever addon create postgresql-addon --plan dev --link cc-scheduler pg-scheduler

# 3. Configurer les variables
clever env set CC_API_TOKEN <votre_token>
clever env set RUST_LOG info

# 4. Déployer
git push clever main
```

## API REST

### Créer un schedule

```http
POST /schedules
Content-Type: application/json

{
  "org_id": "orga_xxxxxxxxxx",
  "app_id": "app_xxxxxxxxxx",
  "name": "API de staging",
  "cron_stop": "0 20 * * 1-5",
  "cron_start": "0 8 * * 1-5",
  "timezone": "Europe/Paris"
}
```

**Comportement** : extinction à 20h, démarrage à 8h, du lundi au vendredi.

### Lister les schedules

```http
GET /schedules
```

### Modifier un schedule

```http
PUT /schedules/:id
Content-Type: application/json

{
  "enabled": false
}
```

### Supprimer un schedule

```http
DELETE /schedules/:id
```

### Déclencher une action immédiate

```http
POST /schedules/:id/trigger/stop
POST /schedules/:id/trigger/start
```

### Lister les apps d'une organisation

```http
GET /orgs/:org_id/apps
```

## Exemples de cron

| Expression        | Signification                        |
|-------------------|--------------------------------------|
| `0 20 * * 1-5`   | 20h, du lundi au vendredi            |
| `0 8 * * 1-5`    | 8h, du lundi au vendredi             |
| `0 22 * * *`     | 22h tous les jours                   |
| `0 0 * * 6,0`    | Minuit le week-end                   |
| `30 7 * * 1-5`   | 7h30, du lundi au vendredi           |

Fuseau horaire : `Europe/Paris` par défaut. Toute timezone IANA est supportée.

## Développement local

```bash
# Copier et remplir les variables
cp .env.example .env

# Lancer une DB locale
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=dev postgres:16

# Lancer l'app
DATABASE_URL=postgres://postgres:dev@localhost/cc_scheduler \
CC_API_TOKEN=<token> \
cargo run
```
