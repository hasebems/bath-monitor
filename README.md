# bath-monitor

Who took a bath today? A Raspberry Pi Pico 2 W in the bathroom reads 5 buttons (one per family member) and a light/occupancy sensor, lights a NeoPixel per button once that person has pressed today, and reports both signals over Wi-Fi to a Flask server on the home LAN, which shows a live dashboard.

See `docs/design.md` for the full design, `docs/protocol.md` for the HTTP contract, and `docs/wiring.md` for pin assignments.

## Server

```bash
cd server
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
python app.py            # http://<this-machine>:8080
```

Tests: `pytest tests/ -q` (from `server/`, with the venv active).

For 24/7 deployment on a Raspberry Pi as a systemd service (production WSGI server via `wsgi.py`, auto-start/restart), see `docs/deploy.md`.

## Firmware

Requires `rustup target add thumbv8m.main-none-eabihf` (RP2350 is Arm Cortex-M33) and `picotool` on PATH for flashing (no `probe-rs`/debug probe needed — flashing is BOOTSEL + `picotool`; `elf2uf2-rs` does not work for RP2350/RP235x).

```bash
cp firmware/src/secrets.rs.example firmware/src/secrets.rs   # fill in real Wi-Fi + server values
cd firmware
cargo build --release     # compile-check
cargo run --release       # build, flash a Pico 2 W in BOOTSEL mode via picotool
```

Logs are over USB serial (`screen /dev/tty.usbmodemXXXX 115200`), not RTT/defmt.

## Status

The server has been deployed and verified on the real Raspberry Pi 4 (dashboard reachable and checked from a phone on the home LAN). The target board changed from Pico W to Pico 2 W (RP2350) before the firmware was ever flashed to real hardware; the firmware has since been migrated to RP2350 (builds and passes `cargo clippy` on `thumbv8m.main-none-eabihf`) but hasn't been flashed to a real Pico 2 W yet, so the buttons/light sensor/NeoPixels/Wi-Fi join are still unverified against real hardware — see `docs/design.md`'s verification section for what to check once wired up.
