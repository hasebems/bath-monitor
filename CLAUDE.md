# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A home IoT system that tracks who has taken a bath. A Raspberry Pi Pico 2 W (RP2350) in the bathroom reads 5 buttons (one per family member, wired GND-to-pin with internal pull-ups) and 1 occupancy light sensor, drives a chain of 5 NeoPixels (one per button, lit while that person has pressed today), plays a beep tone through a MAX98357A I2S amp on every button press, and sends events over Wi-Fi/HTTP to a Flask server on the home LAN, which persists state in SQLite and serves an auto-refreshing HTML dashboard. The server runs 24/7 on a Raspberry Pi 4 as a systemd service. Full design rationale is in `docs/design.md`; the HTTP contract is in `docs/protocol.md`; GPIO/PIO/DMA pin assignments are in `docs/wiring.md`; Pi deployment steps are in `docs/deploy.md`.

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

**Firmware → server contract**: the firmware POSTs `{"person": "<id>"}` to `/api/press` on a debounced button press, and `{"occupied": bool}` to `/api/occupancy` on a debounced occupancy transition. It also polls `GET /api/led-state` (a compact `"10100"`-style string, one char per person) every `config::LED_SYNC_INTERVAL_SECS` to reconcile the NeoPixel chain with the server's state — this is what turns LEDs off after the server's daily reset and restores correct state after a firmware reboot. See `docs/protocol.md` for the full contract; both sides' person lists (`firmware/src/config.rs::PEOPLE` and `server/config.py::PEOPLE`) must stay in the same order.

**Firmware task structure** (`firmware/src/main.rs` spawns all of these): button tasks (one pooled task per person, `buttons.rs`) and the occupancy task (`occupancy.rs`) only ever push events onto `embassy_sync::channel::Channel`s (`events.rs`) — they never touch the network directly. A single `http_client::sender_task` owns the only outbound HTTP client and drains `EVENT_CHANNEL` serially (this is what rate-limits POSTs). `led::led_task` owns the WS2812 chain and drains `LED_CHANNEL` (fed both by button presses, for instant feedback, and by `status_poll::status_poll_task`'s periodic server reconciliation). `audio::audio_task` owns the I2S link to a MAX98357A amp and drains `AUDIO_CHANNEL` (fed only by button presses), replaying a single fixed tone synthesized once at startup (`audio.rs`'s `build_tone`, a sine wavetable walked with a fixed-point phase accumulator) — this task never touches the network either. `net.rs` brings up the cyw43 Wi-Fi chip and returns a `Stack` handle used by both network-facing tasks.

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
- The production Pi runs **Python 3.7.3** (Raspberry Pi OS Buster), so `server/requirements.txt` is pinned to old Flask/Werkzeug/waitress releases and the code avoids Python 3.8+ syntax (`X | Y` types, `list[X]`) via `from __future__ import annotations` — don't add newer type-hint syntax or bump these pins without checking 3.7 compatibility. This dev environment's Python (3.14) can't even import Werkzeug 2.2.3 (removed `ast.Str`), so verifying server changes against the Pi's actual Python version requires a matching interpreter (e.g. `brew install python@3.9` locally as a closer stand-in — 3.7 itself isn't installable via current Homebrew) rather than the default `python3`.
