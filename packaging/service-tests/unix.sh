#!/usr/bin/env bash
# Runs only on disposable CI machines. Verifies real boot-service registration and termination.
set -euo pipefail
service_dir=/opt/pdfcraft-service-test
sudo install -d -m 755 "$service_dir"
sudo install -m 755 target/debug/examples/service_probe "$service_dir/pdfcraft-hot-folder"
probe="$service_dir/pdfcraft-hot-folder"
directory="$RUNNER_TEMP/Hot Folder service test"
mkdir -m 700 "$directory"
config="$directory/tasks.json"
(umask 077; printf '{}\n' > "$config")
cleanup() { sudo "$probe" --service-action uninstall --config "$config" || true; }
trap cleanup EXIT
sudo "$probe" --service-action install --account "$(id -un)" --config "$config"
for attempt in $(seq 1 40); do
  [ -f "$directory/tasks.heartbeat" ] && break
  sleep 0.25
done
test -f "$directory/tasks.heartbeat"
"$probe" --service-action status --config "$config" | grep 'installed=true enabled=true running=true'
sudo "$probe" --service-action disable --config "$config" | grep 'enabled=false'
sudo "$probe" --service-action enable --config "$config" | grep 'enabled=true'
sudo "$probe" --service-action stop --config "$config"
for attempt in $(seq 1 40); do
  [ -f "$directory/tasks.stopped" ] && break
  sleep 0.25
done
test -f "$directory/tasks.stopped"
"$probe" --service-action status --config "$config" | grep 'running=false'
sudo "$probe" --service-action start --config "$config"
sudo "$probe" --service-action uninstall --config "$config"
"$probe" --service-action status --config "$config" | grep 'installed=false'
trap - EXIT
