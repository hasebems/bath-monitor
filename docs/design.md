# Design: bath-monitor — "who took a bath" home IoT system

## Context
`bath-monitor` is a wall-mounted panel in the bathroom with 5 push buttons (one per family member) and 1 occupancy light sensor, wired to a Raspberry Pi Pico W. Pressing a button records "this person bathed today" (idempotent per day, resets at local midnight) — the point is simply to know who has/hasn't pressed today, not to compute "who was last." The light sensor independently reports whether the bath is currently occupied, and that in/out history is kept permanently as a log. The Pico W sends both signals to a Flask server on the home LAN over plain HTTP POST (no MQTT), and the server renders a single auto-refreshing HTML dashboard.

Dev machine (macOS) toolchain used during implementation: rustc/cargo 1.93.0, `thumbv6m-none-eabi` target installed, `elf2uf2-rs`/`flip-link`/`cargo-generate` on PATH (no `probe-rs` — flashing is via BOOTSEL + UF2 drag-and-drop, not SWD debugging). Python 3.14.6/pip 26.1.2.

## Repo layout
Two independent projects in one repo (not a Cargo workspace — only one Rust crate exists):
```
bath-monitor/
├── CLAUDE.md, README.md, .gitignore
├── docs/design.md           # this file
├── docs/protocol.md         # canonical HTTP API contract (source of truth for both sides)
├── docs/wiring.md           # GPIO pin assignments
├── firmware/                 # Rust, embassy-rp, standalone crate
└── server/                   # Python, Flask
```

## Firmware (`firmware/`, Rust + embassy-rp + cyw43)

**Key decisions:**
- Logging: `embassy-usb-logger` (USB-CDC) + `log`, not `defmt`/RTT — no debug probe available, but USB serial works over the same cable used to flash.
- Panic handler: `panic-halt` (RTT-based `panic-probe` is useless without a probe).
- Config: `src/secrets.rs` (gitignored, real Wi-Fi/server values) + `src/secrets.rs.example` (checked in template) + `src/config.rs` (non-secret: person list, GPIO pins, debounce timings).
- HTTP payloads: tiny hand-built JSON via `heapless::String` + `core::write!` (no `serde` — bodies are 1-2 fields).
- Coordination: one `embassy_sync::channel::Channel<AppEvent, 8>` — button/occupancy tasks only ever push events; a single `sender_task` owns the `reqwless::HttpClient` and drains the channel serially (no locking needed, naturally rate-limits POSTs).

**Dependency versions** (verified against crates.io as of 2026-08-30 — double-check the `embassy-net` stack construction API and `cyw43` firmware-blob loading against the live `embassy-rs/embassy` `examples/rp/src/bin/wifi_*.rs` at these exact pinned versions before relying on `net.rs`, since these APIs have changed shape across versions):
```
embassy-executor 0.10.0, embassy-time 0.5.1, embassy-rp 0.10.0,
embassy-net 0.9.1 (tcp, dns, dhcpv4), embassy-sync 0.8.0, embassy-usb-logger 0.6.0,
cyw43 0.7.0, cyw43-pio 0.10.0,
reqwless 0.14.0, embedded-io-async 0.7.0,
cortex-m 0.7.9, cortex-m-rt 0.7.6, panic-halt 1.0.0,
static_cell 2.1.1, heapless 0.9.3, log 0.4.34
```
Also verify `rand_core` version matches whatever `embassy-rp 0.10.0` actually depends on.

**Modules:**
- `main.rs` — executor setup, hardware init, spawns all tasks
- `secrets.rs` / `secrets.rs.example` / `config.rs` — as above
- `events.rs` — `AppEvent { ButtonPressed{person_idx}, OccupancyChanged{occupied} }` + the shared `Channel`
- `net.rs` — cyw43/embassy-net bring-up, Wi-Fi join with exponential-backoff retry (transient AP issues must self-recover — no probe to debug a stuck board)
- `buttons.rs` — 5 async tasks using `Input::wait_for_rising_edge()`, 50ms debounce per pin, push `ButtonPressed`
- `occupancy.rs` — light sensor read (digital `Input` two-state signal per requirements), confirm-stable debounce (~2000ms, to reject chatter near threshold) before pushing `OccupancyChanged`
- `http_client.rs` — owns the single `reqwless::HttpClient`, drains the channel, POSTs `{"person":"alice"}` to `/api/press` or `{"occupied":true}` to `/api/occupancy`, logs outcome via `log::info!`/`warn!`

**Build/flash:** `.cargo/config.toml` sets `target = "thumbv6m-none-eabi"`, `runner = "elf2uf2-rs -d"`, `flip-link` as linker (stack-overflow guard page — valuable with no debugger). `cargo run --release` builds → converts to UF2 → auto-copies to a BOOTSEL-mode Pico.

## Server (`server/`, Python + Flask)

**Stack:** `Flask==3.1.3` only — bundles Jinja2 + a dev server, sufficient for 4 routes and a handful of low-frequency IoT clients. Persistence via stdlib `sqlite3` (schema is 2 small tables — no ORM needed).

**Schema (`schema.sql`):**
```sql
CREATE TABLE press_status (
    person TEXT PRIMARY KEY,
    last_pressed_at TEXT,        -- ISO8601, NULL if never pressed
    last_pressed_date TEXT       -- 'YYYY-MM-DD', drives the reset logic
);
CREATE TABLE occupancy_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occupied INTEGER NOT NULL CHECK (occupied IN (0,1)),
    changed_at TEXT NOT NULL     -- ISO8601, append-only, never pruned
);
```
`press_status` seeded (`INSERT OR IGNORE`) from `config.PEOPLE` at startup — adding a family member is a config change + restart. Current occupancy = the most recent `occupancy_log` row (no separate "current state" table, avoids drift).

**Daily reset (no scheduler):** every read/write compares `last_pressed_date` to `date.today()` (server's local time, stdlib `datetime`, no `pytz`/`zoneinfo` needed since server runs on the home LAN, not UTC cloud infra). `pressed_today = (last_pressed_date == today)`. Press writes are unconditional upserts — same-day re-press is a harmless idempotent overwrite.

**Routes (`app.py`, app-factory pattern):**
| Method | Path | Purpose |
|---|---|---|
| POST | `/api/press` | `{"person": "<id>"}` → validate against `config.PEOPLE` (400 if unknown), upsert `press_status` |
| POST | `/api/occupancy` | `{"occupied": true\|false}` → append to `occupancy_log` (no server-side dedup — trusts firmware debounce) |
| GET | `/api/status` | JSON snapshot: per-person `pressed_today`/`last_pressed_at` + current `occupied`/`occupied_since` |
| GET | `/` | Renders `templates/status.html` using the same `db.get_status()` data as `/api/status` |

`db.py` exposes `init_db()`, `get_status()`, `record_press()`, `record_occupancy()` — routes stay thin (parse → call → return/render).

**Page:** `templates/base.html` + `status.html`, `<meta http-equiv="refresh" content="5">` (no JS/websockets needed for a home LAN dashboard) — 5 people as colored pills (pressed/not), one occupancy banner (occupied/vacant + since-time). `static/style.css` for minimal styling.

## Files created
```
firmware/Cargo.toml, Cargo.lock, .cargo/config.toml, memory.x, build.rs
firmware/src/{main,secrets.rs.example,config,events,net,buttons,occupancy,http_client}.rs
server/{requirements.txt,config.py,schema.sql,db.py,models.py,app.py}
server/templates/{base,status}.html, server/static/style.css
server/tests/{conftest.py,test_api.py}
server/data/.gitkeep   (bath_monitor.db is gitignored runtime state)
docs/design.md, docs/protocol.md, docs/wiring.md
README.md, .gitignore  (target/, secrets.rs, __pycache__/, .venv/, *.db, .DS_Store)
```

## Verification
**Server (no hardware needed):**
```bash
cd server && python -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt
python app.py
# in another terminal:
curl -X POST localhost:8080/api/press -H 'Content-Type: application/json' -d '{"person":"alice"}'
curl -X POST localhost:8080/api/occupancy -H 'Content-Type: application/json' -d '{"occupied":true}'
curl localhost:8080/api/status
open localhost:8080/
```
Also verify: unknown person → 400, malformed body → 400, reset logic by manually backdating `last_pressed_date` in the sqlite file and confirming `/api/status` flips to `false`. Run `pytest server/tests/`.

**Firmware (once flashed):** `cargo run --release` with Pico in BOOTSEL mode → open USB-serial terminal (`screen /dev/tty.usbmodemXXXX 115200`) → confirm Wi-Fi join + IP log, then confirm a physical button press produces a server-side `POST /api/press 200` (visible in Flask's request log) and the dashboard pill flips green within one refresh cycle. Cover/uncover the light sensor and confirm exactly one debounced `occupancy_log` row per real transition, not a chattering flood.
