#!/bin/bash
# =============================================================================
# clever-destroy.sh — Full teardown of a POC Scheduler CC deployment
# =============================================================================
#
# ╔══════════════════════════════════════════════════════════════════╗
# ║  ██╗    ██╗ █████╗ ██████╗ ███╗   ██╗██╗███╗   ██╗ ██████╗    ║
# ║  ██║    ██║██╔══██╗██╔══██╗████╗  ██║██║████╗  ██║██╔════╝    ║
# ║  ██║ █╗ ██║███████║██████╔╝██╔██╗ ██║██║██╔██╗ ██║██║  ███╗   ║
# ║  ██║███╗██║██╔══██║██╔══██╗██║╚██╗██║██║██║╚██╗██║██║   ██║   ║
# ║  ╚███╔███╔╝██║  ██║██║  ██║██║ ╚████║██║██║ ╚████║╚██████╔╝   ║
# ║   ╚══╝╚══╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═══╝╚═╝╚═╝  ╚═══╝ ╚═════╝   ║
# ╠══════════════════════════════════════════════════════════════════╣
# ║  This script PERMANENTLY and IRREVERSIBLY deletes:              ║
# ║    • The Clever Cloud application                               ║
# ║    • The PostgreSQL add-on and all its data                     ║
# ║    • The local .clever.json and git remote                      ║
# ║                                                                  ║
# ║  NO RECOVERY POSSIBLE after confirmation.                       ║
# ║                                                                  ║
# ║  For development and testing only.                              ║
# ╚══════════════════════════════════════════════════════════════════╝
#
# Usage   : bash tools/clever-destroy.sh <app-name> [org-id]
# Example : bash tools/clever-destroy.sh cc-scheduler orga_xxx
# =============================================================================

set -e

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BOLD='\033[1m'; NC='\033[0m'
success() { echo -e "${GREEN}  ✓  $1${NC}"; }
warn()    { echo -e "${YELLOW}  ⚠  $1${NC}"; }
error()   { echo -e "${RED}  ✗  $1${NC}"; exit 1; }

APP="${1:-cc-scheduler}"
ORG_INPUT="$2"
[ -n "$ORG_INPUT" ] && ORG_FLAG="--org $ORG_INPUT" || ORG_FLAG=""

echo ""
echo -e "${BOLD}${RED}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}${RED}║            PERMANENT AND IRREVERSIBLE DELETION                   ║${NC}"
echo -e "${BOLD}${RED}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${RED}  Will be PERMANENTLY deleted:${NC}"
echo -e "${RED}    • Application  : $APP${NC}"
echo -e "${RED}    • PostgreSQL   : ${APP}-pg  (all schedule and log data)${NC}"
echo -e "${RED}    • Local        : .clever.json + git remote CC-Scheduler${NC}"
echo ""
echo -e "${BOLD}${RED}  ⚠  This operation is IRREVERSIBLE. No recovery possible.${NC}"
echo ""
echo -ne "${BOLD}${RED}  Type exactly 'delete' to confirm: ${NC}"
read -r CONFIRM
[ "$CONFIRM" != "delete" ] && echo "" && warn "Cancelled — no resources deleted." && exit 0
echo ""

clever addon delete "${APP}-pg" --yes 2>/dev/null && success "${APP}-pg deleted." || warn "${APP}-pg not found."
clever delete --app "$APP"      --yes 2>/dev/null && success "$APP deleted."      || warn "$APP not found."
git remote remove CC-Scheduler  2>/dev/null        && success "Remote CC-Scheduler removed." || warn "Remote CC-Scheduler not found."
rm -f .clever.json && success ".clever.json removed."

echo ""
echo -e "${GREEN}  Done. Run deploy/clever-deploy.sh to start fresh.${NC}"
echo ""
