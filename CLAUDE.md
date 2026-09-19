# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A home IoT system that tracks who has taken a bath. A Raspberry Pi Pico 2 W (RP2350) in the bathroom reads 5 buttons (one per family member, wired GND-to-pin with internal pull-ups) and 1 occupancy light sensor, drives a chain of 5 NeoPixels (one per button, lit while that person has pressed today), plays a per-person melody through a MAX98357A I2S amp on every button press, and sends events over Wi-Fi/HTTP to a Flask server on the home LAN, which persists state in SQLite and serves an auto-refreshing HTML dashboard. The server runs 24/7 on a Raspberry Pi 4 as a systemd service. Full design rationale is in `docs/design.md`; the HTTP contract is in `docs/protocol.md`; GPIO/PIO/DMA pin assignments are in `docs/wiring.md`; Pi deployment steps are in `docs/deploy.md`.

Two independent projects share this repo (not a Cargo workspace):
- `firmware/` — Rust, `no_std`, embassy-rp + cyw43, targets the Pico 2 W (RP2350)
- `server/` — Python, Flask

## Commands

### Server (`server/`)

```bash
cd server
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
python app.py                      # dev server (Flask/Werkzeug), runs on 0.0.0.0:8080
python wsgi.py                     # production server (waitress) — what systemd runs on the Pi
```

Tests (pytest, Flask test client, temp-file SQLite per test):
```bash
cd server && source .venv/bin/activate
pytest tests/ -q                   # all tests
pytest tests/test_api.py::test_press_is_idempotent_same_day -q   # single test
```

No linter is configured for the server yet. See `docs/deploy.md` for the systemd unit (`server/deploy/bath-monitor.service`) that runs `wsgi.py` on boot with auto-restart.

### Firmware (`firmware/`)

Requires `rustup target add thumbv8m.main-none-eabihf` (RP2350 is Arm Cortex-M33, not the RP2040's Cortex-M0+) and `picotool` on PATH for flashing (`elf2uf2-rs` does not produce a working UF2 for RP235x — see Notes for future work). Before building, copy `firmware/src/secrets.rs.example` to `firmware/src/secrets.rs` (gitignored) and fill in real Wi-Fi/server values.

```bash
cd firmware
cargo build --release              # compile-check without hardware
cargo run --release                # build, flash a Pico 2 W in BOOTSEL mode via picotool
cargo clippy --release              # lint
```

There is no hardware-in-the-loop test suite — verify by flashing and watching USB-serial logs (`screen /dev/tty.usbmodemXXXX 115200`; logging is via `embassy-usb-logger`, not RTT/defmt, since there's no debug probe in this setup).

## Architecture

**Firmware → server contract**: the firmware POSTs `{"person": "<id>"}` to `/api/press` on a debounced button press, and `{"occupied": bool}` to `/api/occupancy` on a debounced occupancy transition — but only while Wi-Fi is connected; anything owed to the server while offline (or while the server is down) is kept in `outbox.rs` and re-sent until acknowledged (presses: per-person, re-sent until 2xx; occupancy: only the current value, and only if it differs from the last one delivered). It also polls `GET /api/led-state` (a compact `"10100"`-style string, one char per person) every `config::LED_SYNC_INTERVAL_SECS` to reconcile the NeoPixel chain with the server's state — this is what turns LEDs off after the server's daily reset and restores correct state after a firmware reboot (people whose press is still undelivered stay lit). See `docs/protocol.md` for the full contract; both sides' person lists (`firmware/src/config.rs::PEOPLE` and `server/config.py::PEOPLE`) must stay in the same order.

**Firmware task structure** (`firmware/src/main.rs` spawns all of these): **nothing but the network tasks depends on Wi-Fi** — `main.rs` starts the LED, audio, occupancy and button tasks first and only then brings up the Wi-Fi chip, so the hardware works whether or not Wi-Fi ever connects (see `docs/additional_spec.md`, "Wi-Fi接続状態に依存しない動作"). Button tasks (one pooled task per person, `buttons.rs`) and the occupancy task (`occupancy.rs`) never touch the network and never block on it: they record what the server is owed in `outbox.rs` (plain atomics + a `Signal`, not a queue) and do their local reaction (`events::LED_CHANNEL`, `events::MELODY_CHANNEL`). `wifi::wifi_task` owns cyw43's `Control`, joins (with backoff) and re-joins Wi-Fi, and publishes the result as the global `wifi::is_connected()` (link up *and* DHCP done); a single `http_client::sender_task` owns the only outbound POST client and delivers `outbox` contents serially, only while connected (this is what rate-limits POSTs), with a per-request timeout and retry on failure. `led::led_task` owns the WS2812 chain and drains `LED_CHANNEL` (fed both by button presses, for instant feedback, and by `status_poll::status_poll_task`'s periodic server reconciliation). Audio runs entirely on CORE1 (`audio::start`, three tasks: `audio::core1_task` owns the I2S link and DMAs whatever's in `WAVEFORM_BUFFER` out in a loop; `waveform::waveform_task` synthesizes each chunk from 2 oscillator+envelope slots, paced by `audio::BUFFER_CONSUMED`; `music::music_task` schedules one of 5 fixed melodies' notes out to those slots) — none of these three touch the network. A button press reaches CORE1 via `events::MELODY_CHANNEL` (the one channel in `events.rs` that crosses cores), selecting which person's melody plays; see `docs/additional_spec.md` for the full synth design. `net.rs` only brings up the cyw43 chip and the embassy-net `Stack` (usable before Wi-Fi joins) and returns them plus the cyw43 `Control`, which goes to `wifi_task`. `wifi_task` also blinks the Pico 2 W's onboard LED (wired to the CYW43439's `WL_GPIO0`, not an RP2350 GPIO, so it's only reachable via `Control`, which `join` also needs `&mut` access to) at 1Hz as a liveness indicator — held on during a `join` call, blinking otherwise; it stops if CORE0's executor hangs/panics. Reconnection after a link drop (a plain `join` again, no `leave` in between) and everything else about this Wi-Fi/outbox layer is build/clippy-clean but not yet flash-tested.

Peripheral allocation is fixed and hand-wired in `main.rs` (GPIO/PIO fields can't be indexed dynamically from the `config.rs` arrays at runtime): PIO0 + DMA_CH0 for the cyw43 SPI link, PIO1 + DMA_CH2 for the NeoPixel chain, PIO2 + DMA_CH3 for the MAX98357A I2S output (`irqs.rs` binds all interrupt vectors in one place). If you change a pin or add/remove a family member, update `config.rs`, the hardcoded `p.PIN_N` calls in `main.rs`, `server/config.py::PEOPLE`, and `docs/wiring.md` together.

**Server**: `app.py` (routes) → `db.py` (all SQLite access) → `schema.sql` (2 tables: `press_status` per-person with lazy date-comparison reset, `occupancy_log` append-only). `get_status()`/`get_led_state()` are the single source of truth consumed by `/`, `/api/status`, and `/api/led-state` alike — don't duplicate that query logic in routes.

## Notes for future work

- Crate versions in `firmware/Cargo.toml` were verified against the actual published crates.io releases (not the embassy-rs/embassy "main" branch examples, which track unreleased APIs) as of 2026-08-30 — if bumping embassy-* versions, expect API drift (this happened repeatedly during initial implementation: `Stack::new` → free function `embassy_net::new`, `DnsClient` → `DnsSocket`, `PioWs2812::write_slice` → `write`, etc.) and re-check against the actual crate source in `~/.cargo/registry/src/*/<crate>-<version>/` rather than trusting example code from the git repo.
- `reqwless` is a plain crates.io dependency (0.14.0), not git-pinned — it targets `embedded-nal-async` trait bounds, which the published `embassy-net` satisfies directly.
- **Pico W → Pico 2 W (RP2350) migration** (decided 2026-09-03, firmware code migrated same day — `cargo build --release`/`cargo clippy --release` both pass on `thumbv8m.main-none-eabihf`, but not yet flash-tested on real Pico 2 W hardware). The two boards are pin-compatible (same 40-pin layout, same GPIO numbering), so `docs/wiring.md`'s GPIO table and `config.rs::BUTTON_PINS`/`LIGHT_SENSOR_PIN` needed no changes. What changed:
  - `.cargo/config.toml`: target `thumbv6m-none-eabi` → `thumbv8m.main-none-eabihf` (RP2350's Cortex-M33 core); runner `elf2uf2-rs -d` → `picotool load -u -v -x -t elf` (`elf2uf2-rs` doesn't produce a working UF2 for RP235x — confirmed via embassy-rs/embassy#4322); kept `linker = "flip-link"` — confirmed it still runs and links successfully on this target (verified via `cargo build -v`), though the resulting flipped-stack layout is not yet hardware-tested.
  - `firmware/Cargo.toml`: `embassy-rp` feature `"rp2040"` → `"rp235xa"` (Pico 2 W uses the RP2350A/QFN-60 die, same as Pico 2 — not `"rp235xb"`, which is for the QFN-80 variant with more GPIO).
  - `firmware/memory.x`: replaced with RP2350's layout (`FLASH`/`RAM`/`SRAM8`/`SRAM9` regions + `.start_block`/`.bi_entries`/`.end_block` SECTIONS, copied from `embassy-rs/embassy`'s `examples/rp235x/memory.x`) since RP2350 has no RP2040-style BOOT2 stage — its ROM bootloader instead requires a boot Image Definition block linked into the first 4K of flash. No code changes were needed in `main.rs` for this: embassy-rp's `rp235xa` feature emits the `IMAGE_DEF` static itself (confirmed present at `__start_block_addr` via `nm` on the built ELF). Flash sized to the real Pico 2 W's 4MB (vs Pico W's 2MB); RAM to RP2350's 520KB (vs RP2040's 264KB).
  - `firmware/build.rs`: dropped the `-Tlink-rp.x` linker arg — that script is RP2040-specific (embassy's own `examples/rp235x/build.rs` omits it; including it would fail to find the script on RP235x).
  - `firmware/src/net.rs`: `cyw43_pio::DEFAULT_CLOCK_DIVIDER` → `RM2_CLOCK_DIVIDER` for the `PioSpi::new` clock divider — embassy's Pico 2 W example uses this because `DEFAULT_CLOCK_DIVIDER` can leave cyw43 SPI unreliable on RP2350 (embassy-rs/embassy#3960).
  - `firmware/cyw43-firmware/nvram_rp2040.bin`: kept unchanged/unrenamed — same CYW43439 chip as Pico W, and embassy's own rp235x/Pico 2 W example (`examples/rp235x/src/bin/blinky_wifi.rs`) loads this identical file, so it's confirmed reusable despite the RP2040-suggesting filename.
  - PIO/DMA allocation (PIO0+DMA_CH0 for cyw43, PIO1+DMA_CH2 for the NeoPixel chain) was left as-is and still builds cleanly — RP2350 has more PIO blocks (3 vs 2) and DMA channels (16 vs 12) than RP2040, so there's no conflict, just less resource pressure than before.
  - Not yet done: flashing and verifying against a real Pico 2 W board (buttons/light sensor/NeoPixels/Wi-Fi join all still unverified on hardware, same as before this migration).
- **Audio synth** (`firmware/src/waveform.rs`/`music.rs`, implemented 2026-09-18 per `docs/additional_spec.md`; `cargo build --release`/`cargo clippy --release` both pass, not yet flash-tested on real hardware — same caveat as the rest of this firmware). Two open items intentionally left for later, not blocking on this implementation:
  - `config.rs`'s `AUDIO_ATTACK_RATE`/`AUDIO_RELEASE_RATE`/`AUDIO_DAMP_RATE`/`AUDIO_MINIMUM_LEVEL` are placeholder values, not yet tuned by ear against real hardware.
  - `music.rs`'s 5 melodies (`MELODY_0`..`MELODY_4`) are placeholder motifs (just distinct enough to exercise the playback module), not real compositions.
- The production Pi runs **Python 3.7.3** (Raspberry Pi OS Buster), so `server/requirements.txt` is pinned to old Flask/Werkzeug/waitress releases and the code avoids Python 3.8+ syntax (`X | Y` types, `list[X]`) via `from __future__ import annotations` — don't add newer type-hint syntax or bump these pins without checking 3.7 compatibility. This dev environment's Python (3.14) can't even import Werkzeug 2.2.3 (removed `ast.Str`), so verifying server changes against the Pi's actual Python version requires a matching interpreter (e.g. `brew install python@3.9` locally as a closer stand-in — 3.7 itself isn't installable via current Homebrew) rather than the default `python3`.
