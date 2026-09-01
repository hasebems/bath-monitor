# Deploying the server on Raspberry Pi 4 (24/7)

Target: Raspberry Pi OS (Debian-based, systemd), running the server continuously as a systemd service that starts on boot and restarts on crash. The server is pure Python + stdlib `sqlite3`, so no architecture-specific steps are needed.

## Install

```bash
git clone https://github.com/hasebems/bath-monitor.git ~/bath-monitor
cd ~/bath-monitor/server
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

Adjust paths below if you clone somewhere other than `/home/pi/bath-monitor` or run as a user other than `pi`.

## systemd service

`server/deploy/bath-monitor.service` is the unit template — it runs `server/wsgi.py` (waitress, a production WSGI server) rather than `app.py`'s Flask dev server, since this runs unattended long-term.

```bash
sudo cp ~/bath-monitor/server/deploy/bath-monitor.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now bath-monitor
```

Check status / logs:
```bash
systemctl status bath-monitor
journalctl -u bath-monitor -f
```

The dashboard is then reachable at `http://<pi-hostname-or-ip>:8080/` from any device on the home LAN — point the firmware's `SERVER_BASE_URL` (`firmware/src/secrets.rs`) at that address.

## Updating after a code change

```bash
cd ~/bath-monitor && git pull
cd server && .venv/bin/pip install -r requirements.txt   # only if requirements.txt changed
sudo systemctl restart bath-monitor
```

## Notes

- `server/data/bath_monitor.db` persists across restarts (it's on local disk, gitignored). Back it up if you care about historical `occupancy_log` data.
- The daily press-status reset relies on the Pi's system clock/timezone being correct for the household (`date.today()` uses local time) — verify with `timedatectl` if presses seem to reset at the wrong hour.
- `Restart=always` in the unit file means a crash or `journalctl`-visible exception gets the service back up within `RestartSec=5` without manual intervention.
