#!/usr/bin/env bash
# Register the IAP OAuth client (a "Web application" client created in the console) without printing the client secret.
# Usage: PROJECT=my-project deploy/iap-set-client.sh ~/Downloads/client_secret_XXXX.json
# Needed once in a project without an organization, where IAP cannot create its OAuth client automatically.
set -euo pipefail
f=${1:?pass the downloaded client JSON}
: "${PROJECT:?set PROJECT}"
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
python3 - "$f" "$tmp" <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
c = c.get('web') or c.get('installed') or c
open(sys.argv[2], 'w').write(
  "access_settings:\n  oauth_settings:\n"
  f"    client_id: {c['client_id']}\n    client_secret: {c['client_secret']}\n")
print('client id: ' + c['client_id'])
PY
# The OAuth client is a project-level setting (iap_web); service-level settings reject it.
gcloud iap settings set "$tmp" --project="$PROJECT" --resource-type=iap_web --format=none
echo "registered"
