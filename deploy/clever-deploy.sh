#!/bin/bash
# =============================================================================
# clever-deploy.sh — Automated deployment of POC Scheduler CC on Clever Cloud
# =============================================================================

set -e

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; CYAN='\033[0;36m'; BOLD='\033[1m'; DIM='\033[2m'; NC='\033[0m'

info()    { echo -e "${BLUE}  ℹ  $1${NC}"; }
success() { echo -e "${GREEN}  ✓  $1${NC}"; }
warn()    { echo -e "${YELLOW}  ⚠  $1${NC}"; }
error()   { echo -e "${RED}  ✗  $1${NC}"; exit 1; }
section() { echo -e "\n${BOLD}${BLUE}▶ $1${NC}\n"; }

ask() {
    echo -e "${CYAN}  ?  $1${NC}"
    echo -ne "${BOLD}      → ${NC}"
}

menu() {
    local title="$1"; shift
    local default_idx="$1"; shift
    local options=("$@")
    echo -e "${CYAN}  ?  $title${NC}"
    for i in "${!options[@]}"; do
        local num=$((i + 1))
        if [ "$i" -eq "$((default_idx - 1))" ]; then
            echo -e "      ${BOLD}${GREEN}$num) ${options[$i]} ★ recommended${NC}"
        else
            echo -e "      ${DIM}$num) ${options[$i]}${NC}"
        fi
    done
    echo -ne "${BOLD}      → ${NC}"
}

# -----------------------------------------------------------------------------
# Auto-cleanup on error
# -----------------------------------------------------------------------------
cleanup_on_error() {
    echo ""
    warn "Error detected — cleaning up..."
    [ -n "$PG_ADDON_NAME" ] && clever addon delete "$PG_ADDON_NAME" --yes 2>/dev/null || true
    [ -n "$APP_ALIAS" ]     && clever delete --app "$APP_ALIAS" --yes 2>/dev/null || true
    git remote remove CC-Scheduler 2>/dev/null || true
    rm -f .clever.json
    warn "Cleanup done."
}
trap cleanup_on_error ERR

# =============================================================================
# HEADER
# =============================================================================
echo ""
echo -e "${BOLD}${BLUE}╔══════════════════════════════════════════╗${NC}"
echo -e "${BOLD}${BLUE}║     POC Scheduler CC — Clever Cloud       ║${NC}"
echo -e "${BOLD}${BLUE}╚══════════════════════════════════════════╝${NC}"
echo ""

# =============================================================================
# PREREQUISITES
# =============================================================================
section "Checking prerequisites"
command -v clever >/dev/null 2>&1 || error "clever-tools not installed: npm install -g clever-tools"
command -v git    >/dev/null 2>&1 || error "git is not installed."
command -v curl   >/dev/null 2>&1 || error "curl is not installed."
clever profile >/dev/null 2>&1    || error "Not logged in. Run: clever login"
git rev-parse --git-dir >/dev/null 2>&1 || error "Run this script from the root of the git repository."
success "Prerequisites OK."

# =============================================================================
# GENERAL CONFIGURATION
# =============================================================================
section "General configuration"

ask "Application name [default: cc-scheduler]:"
read -r APP_NAME
APP_NAME="${APP_NAME:-cc-scheduler}"
APP_ALIAS="$APP_NAME"

echo ""
ask "Clever Cloud organisation ID (Enter = personal account):"
read -r ORG_INPUT
if [ -n "$ORG_INPUT" ]; then
    ORG_FLAG="--org $ORG_INPUT"
    info "Organisation: $ORG_INPUT"
else
    ORG_FLAG=""
    info "Personal account selected."
fi

# Region
echo ""
REGIONS=(
    "Paris     (par) — Europe, France"
    "Roubaix   (rbx) — Europe, France"
    "Warsaw    (wsw) — Europe, Poland"
    "London    (ldn) — Europe, UK"
    "Montreal  (mtl) — North America"
    "Singapore (sgp) — Asia-Pacific"
    "Sydney    (syd) — Asia-Pacific"
)
REGION_CODES=("par" "rbx" "wsw" "ldn" "mtl" "sgp" "syd")
menu "Deployment region:" 1 "${REGIONS[@]}"
read -r RC
RC="${RC:-1}"
REGION="${REGION_CODES[$((RC - 1))]:-par}"
info "Region: $REGION"

# =============================================================================
# POSTGRESQL
# =============================================================================
section "PostgreSQL add-on"

PG_PLANS=(
    "dev     —  shared, free  (dev / POC)"
    "xxs_sml —  1 vCPU,  512 MB RAM,  1 GB  (small team)"
    "xs_sml  —  1 vCPU,  1 GB RAM,    5 GB  (standard)"
)
PG_PLAN_CODES=("dev" "xxs_sml" "xs_sml")
menu "PostgreSQL plan:" 1 "${PG_PLANS[@]}"
read -r PGC
PGC="${PGC:-1}"
PG_PLAN="${PG_PLAN_CODES[$((PGC - 1))]:-dev}"
info "PostgreSQL plan: $PG_PLAN"

# =============================================================================
# SCHEDULER CONFIGURATION
# =============================================================================
section "Scheduler configuration"

ask "Web UI password (required):"
read -s -r APP_PASSWORD
echo ""
[ -z "$APP_PASSWORD" ] && error "Password is required."

ask "Confirm password:"
read -s -r APP_PASS_CONFIRM
echo ""
[ "$APP_PASSWORD" != "$APP_PASS_CONFIRM" ] && error "Passwords do not match."

# =============================================================================
# SUMMARY
# =============================================================================
section "Summary"
echo -e "  ${DIM}Application ${NC}  ${BOLD}$APP_NAME${NC} — region ${BOLD}$REGION${NC}"
echo -e "  ${DIM}PostgreSQL  ${NC}  ${BOLD}$PG_PLAN${NC}"
echo -e "  ${DIM}Organisation${NC}  ${BOLD}${ORG_INPUT:-personal account}${NC}"
echo ""
ask "Confirm deployment? (y/N):"
read -r CONFIRM
[[ "$CONFIRM" =~ ^[yYoO]$ ]] || { trap - ERR; warn "Cancelled."; exit 0; }

# =============================================================================
# CREATE APPLICATION
# =============================================================================
section "Creating Rust application"
clever create --type rust --region "$REGION" $ORG_FLAG --alias "$APP_ALIAS" "$APP_NAME"
success "Application $APP_NAME created."

# =============================================================================
# CREATE POSTGRESQL ADD-ON
# =============================================================================
section "Creating PostgreSQL add-on"
PG_ADDON_NAME="${APP_NAME}-pg"
if [ "$PG_PLAN" = "dev" ]; then
    clever addon create postgresql-addon --plan "$PG_PLAN" --region "$REGION" \
        $ORG_FLAG --link "$APP_ALIAS" "$PG_ADDON_NAME" --yes >/dev/null 2>&1
else
    clever addon create postgresql-addon --plan "$PG_PLAN" --region "$REGION" \
        $ORG_FLAG --link "$APP_ALIAS" "$PG_ADDON_NAME" --yes >/dev/null 2>&1
fi
success "PostgreSQL add-on $PG_ADDON_NAME created and linked."

# =============================================================================
# CREATE SERVICE TOKEN
# =============================================================================
section "Creating Clever Cloud service token"

if [ -n "$ORG_INPUT" ]; then
    EXPIRY="$(date -v+1y '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || date -d '+1 year' '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null)"
    TOKEN_RESPONSE=$(clever curl -s -X POST \
        -H "Content-Type: application/json" \
        -d "{\"name\":\"${APP_NAME}\",\"role\":\"MANAGER\",\"expirationDate\":\"${EXPIRY}\"}" \
        "https://api.clever-cloud.com/v2/organisations/${ORG_INPUT}/service-tokens" 2>/dev/null)

    CC_SERVICE_TOKEN=$(echo "$TOKEN_RESPONSE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['token'])" 2>/dev/null)

    if [ -z "$CC_SERVICE_TOKEN" ]; then
        warn "Could not create service token automatically."
        warn "Create one manually in the Clever Cloud console and paste it below."
        ask "CC_SERVICE_TOKEN:"
        read -r CC_SERVICE_TOKEN
        [ -z "$CC_SERVICE_TOKEN" ] && error "Service token is required."
    else
        success "Service token created (expires in 1 year)."
    fi

    clever env set --alias "$APP_ALIAS" CC_ORG_ID        "$ORG_INPUT"
    clever env set --alias "$APP_ALIAS" CC_SERVICE_TOKEN "$CC_SERVICE_TOKEN"
else
    warn "No organisation provided — CC_SERVICE_TOKEN must be set manually."
    ask "CC_ORG_ID (required even for personal account):"
    read -r MANUAL_ORG
    [ -z "$MANUAL_ORG" ] && error "CC_ORG_ID is required."
    ask "CC_SERVICE_TOKEN:"
    read -r CC_SERVICE_TOKEN
    [ -z "$CC_SERVICE_TOKEN" ] && error "Service token is required."
    clever env set --alias "$APP_ALIAS" CC_ORG_ID        "$MANUAL_ORG"
    clever env set --alias "$APP_ALIAS" CC_SERVICE_TOKEN "$CC_SERVICE_TOKEN"
fi

clever env set --alias "$APP_ALIAS" APP_PASSWORD "$APP_PASSWORD"
success "Environment variables configured."

# =============================================================================
# DEPLOY
# =============================================================================
section "Deploying"
info "Pushing source code..."
clever deploy --alias "$APP_ALIAS"

trap - ERR

# =============================================================================
# SUCCESS
# =============================================================================
APP_URL=$(clever domain --alias "$APP_ALIAS" 2>/dev/null | grep 'cleverapps.io' | awk '{print $1}' | head -n1)
[ -z "$APP_URL" ] && APP_URL="${APP_NAME}.cleverapps.io"

echo ""
echo -e "${BOLD}${GREEN}╔══════════════════════════════════════════╗${NC}"
echo -e "${BOLD}${GREEN}║            Deployment successful!         ║${NC}"
echo -e "${BOLD}${GREEN}╚══════════════════════════════════════════╝${NC}"
echo ""
echo -e "  ${DIM}URL    ${NC}  ${BOLD}${GREEN}https://${APP_URL}${NC}"
echo -e "  ${DIM}Logs   ${NC}  clever logs --alias $APP_ALIAS"
echo -e "  ${DIM}Destroy${NC}  bash tools/clever-destroy.sh $APP_NAME ${ORG_INPUT}"
echo ""
