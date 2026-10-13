#!/usr/bin/env bash
# Envia o código para o servidor, compila lá (x86_64 Linux) e reinicia o serviço.
# Uso: ./deploy.sh
set -euo pipefail
HOST=${HOST:-sublime-eu}
APP=pdm-api

ssh "$HOST" "mkdir -p src/$APP"
rsync -az --delete --exclude target --exclude media --exclude .env --exclude .git ./ "$HOST:src/$APP/"
ssh "$HOST" bash -s <<REMOTE
set -euo pipefail
cd ~/src/$APP
~/.cargo/bin/cargo build --release --locked -q
sudo install -m 755 target/release/$APP /opt/$APP/$APP
sudo rsync -a --delete static/ /opt/$APP/static/
sudo systemctl enable --now $APP >/dev/null 2>&1 || true
sudo systemctl restart $APP
sleep 1
systemctl is-active $APP
REMOTE
