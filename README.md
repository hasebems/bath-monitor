# bath-monitor

Who took a bath today? A Raspberry Pi Pico W in the bathroom reads 5 buttons (one per family member) and a light/occupancy sensor, lights a NeoPixel per button once that person has pressed today, and reports both signals over Wi-Fi to a Flask server on the home LAN, which shows a live dashboard.

See `docs/design.md` for the full design, `docs/protocol.md` for the HTTP contract, and `docs/wiring.md` for pin assignments.

## Server

```bash
cd server
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
python app.py            # http://<this-machine>:8080
```

Tests: `pytest tests/ -q` (from `server/`, with the venv active).

## Firmware

Requires `rustup target add thumbv6m-none-eabi` and `cargo install elf2uf2-rs flip-link` (no `probe-rs`/debug probe needed — flashing is BOOTSEL + UF2).

```bash
cp firmware/src/secrets.rs.example firmware/src/secrets.rs   # fill in real Wi-Fi + server values
cd firmware
cargo build --release     # compile-check
cargo run --release       # build, convert to UF2, flash a Pico W in BOOTSEL mode
```

Logs are over USB serial (`screen /dev/tty.usbmodemXXXX 115200`), not RTT/defmt.

## Status

Server and firmware both build and pass their checks (`pytest`, `cargo build`, `cargo clippy`) in this environment, but neither has been run against real hardware (Pico W, buttons, light sensor, NeoPixels) yet — see `docs/design.md`'s verification section for what to check once wired up.
