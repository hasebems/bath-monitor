# Wiring

One PCB mounted in/near the bathroom, wired to a single Raspberry Pi Pico W. Pin numbers below are GPIO numbers and must match `firmware/src/config.rs`.

## Buttons (5x, one per family member)

Each button: one leg to the GPIO pin (configured as input with internal pull-up), the other leg to GND. A press pulls the pin low. Order in `PEOPLE`/`BUTTON_PINS` must match `server/config.py`'s `PEOPLE` list.

| Person index | GPIO |
|---|---|
| 0 | 2 |
| 1 | 3 |
| 2 | 4 |
| 3 | 5 |
| 4 | 6 |

Adjust to actual wiring before flashing.

## Light / occupancy sensor (1x)

Assumed to present a clean two-state digital signal (e.g. via a comparator/threshold circuit on the sensor board) on GPIO 26, read as a plain digital input. If the sensor instead outputs an analog level, `firmware/src/occupancy.rs` will need to switch to `embassy_rp::adc::Adc` with a threshold constant instead of a digital `Input` — update this doc and `config.rs`'s `LIGHT_SENSOR_PIN` comment accordingly if that's the case.

Suggested part: a CdS photoresistor (LDR) + LM393 comparator "light detection module" (common, cheap, sold for Arduino) — it has an onboard trim-pot threshold and outputs HIGH/LOW directly, matching the digital-input assumption above with no firmware changes. Not yet ordered/confirmed against real bathroom light levels — verify the trim-pot threshold once installed.

| Signal | GPIO |
|---|---|
| Occupancy sensor | 26 |

## NeoPixel (WS2812) press indicators (5x, daisy-chained)

One NeoPixel per button, wired in a single chain (`DIN` of LED 0 → GPIO data pin; `DOUT` of LED N → `DIN` of LED N+1), same order as `BUTTON_PINS`/`PEOPLE`. Driven via PIO1 (PIO0 is used by the cyw43 Wi-Fi SPI link) + DMA_CH2 (DMA_CH0/CH1 are used by cyw43). Lit (green) when that person has pressed today; reconciled periodically from the server's `/api/led-state` (see `firmware/src/status_poll.rs`, `firmware/src/led.rs`).

| Signal | GPIO |
|---|---|
| NeoPixel data (chain of 5) | 15 |

## PIO / DMA allocation summary

| Resource | Used by |
|---|---|
| PIO0, DMA_CH0 | cyw43 Wi-Fi SPI (`net.rs`) |
| PIO1, DMA_CH2 | WS2812 NeoPixel chain (`led.rs`) |
| USB | `embassy-usb-logger` (serial log output) |
