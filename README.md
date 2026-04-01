# CC Scheduler

Interface web de gestion des horaires d'extinction/démarrage automatique d'applications Clever Cloud.

Chaque instance gère **une organisation CC**. L'authentification repose sur un **service token Biscuit** pour l'API CC, et un mot de passe pour l'interface web.

## Stack

- **Rust** + **Axum** (API REST + interface web)
- **tokio-cron-scheduler** (exécution des tâches cron)
- **PostgreSQL** (persistance des schedules, add-on CC)
- **Service tokens Biscuit** (auth API Clever Cloud)

## Fonctionnalités

- Sidebar listant les applications de l'organisation
- Création de schedules stop/start avec expression cron et fuseau horaire
- Activation/désactivation à la volée
- Déclenchement manuel immédiat (start / stop)
- Interface web protégée par mot de passe

## Déploiement sur Clever Cloud

### 1. Créer l'application et l'add-on PostgreSQL

```bash
clever create --type rust cc-scheduler --region par --org <org_id>
clever addon create postgresql-addon --plan dev --link cc-scheduler
```

### 2. Créer un service token pour votre organisation

```bash
# Avec clever-tools (nécessite d'être connecté)
curl -X POST https://api.clever-cloud.com/v2/organisations/<org_id>/service-tokens \
  -H "Authorization: Bearer <votre_token_perso>" \
  -H "Content-Type: application/json" \
  -d '{"name": "cc-scheduler", "role": "MANAGER", "ttl_seconds": 31536000}'
```

Ou via un script Python en utilisant vos credentials OAuth1 clever-tools.

### 3. Configurer les variables d'environnement

```bash
clever env set CC_ORG_ID      "orga_xxxxxxxxxxxxxxxxxxxxxxxxxx"
clever env set CC_SERVICE_TOKEN "<biscuit_token>"
clever env set APP_PASSWORD   "<mot_de_passe_interface_web>"
```

| Variable               | Description                                   | Injectée par CC |
|------------------------|-----------------------------------------------|-----------------|
| `PORT`                 | Port HTTP (8080 par défaut)                   | ✓               |
| `POSTGRESQL_ADDON_URI` | URL de connexion PostgreSQL                   | ✓ (add-on)      |
| `CC_ORG_ID`            | ID de l'organisation CC à gérer               | À configurer    |
| `CC_SERVICE_TOKEN`     | Service token Biscuit (rôle MANAGER minimum)  | À configurer    |
| `APP_PASSWORD`         | Mot de passe de l'interface web               | À configurer    |
| `RUST_LOG`             | Niveau de log (ex: `info`)                    | À configurer    |

### 4. Déployer

```bash
git push origin main
clever deploy
```

## Développement local

```bash
# Base de données locale
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=dev postgres:16

# Lancer l'app
DATABASE_URL=postgres://postgres:dev@localhost/cc_scheduler \
CC_ORG_ID=orga_xxx \
CC_SERVICE_TOKEN=<biscuit> \
APP_PASSWORD=monmotdepasse \
cargo run
```

L'interface est accessible sur http://localhost:8080 — le login demande `APP_PASSWORD`.

## API REST

Toutes les routes sont protégées par le cookie de session (login requis).

### Schedules

```http
GET    /schedules                        # Lister
POST   /schedules                        # Créer
GET    /schedules/:id                    # Détail
PUT    /schedules/:id                    # Modifier
DELETE /schedules/:id                    # Supprimer
POST   /schedules/:id/trigger/start      # Démarrer maintenant
POST   /schedules/:id/trigger/stop       # Éteindre maintenant
```

**Créer un schedule :**

```json
POST /schedules
{
  "org_id": "orga_xxx",
  "app_id": "app_xxx",
  "name": "Staging nuit",
  "cron_stop":  "0 20 * * 1-5",
  "cron_start": "0 8 * * 1-5",
  "timezone": "Europe/Paris",
  "enabled": true
}
```

### Clever Cloud proxy

```http
GET /orgs                   # Organisation configurée
GET /orgs/:id/apps          # Applications de l'organisation
```

## Exemples de cron

| Expression       | Signification                      |
|------------------|------------------------------------|
| `0 20 * * 1-5`  | 20h, lundi–vendredi                |
| `0 8 * * 1-5`   | 8h, lundi–vendredi                 |
| `0 22 * * *`    | 22h tous les jours                 |
| `0 0 * * 6,0`   | Minuit le week-end                 |
| `30 7 1 * *`    | 7h30 le 1er de chaque mois         |

Timezone IANA supportée (ex: `Europe/Paris`, `UTC`, `America/New_York`).

## Sécurité

- **Interface web** : protégée par mot de passe (`APP_PASSWORD`), session cookie HttpOnly HMAC-SHA1 (7 jours)
- **API CC** : service token Biscuit org-scoped, révocable depuis la console CC
- **Isolation** : un déploiement = une organisation = un token dédié
