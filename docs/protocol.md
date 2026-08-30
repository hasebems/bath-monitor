# HTTP protocol

Canonical contract between `firmware/` (client) and `server/` (Flask app). Both sides must match this — the firmware's `PEOPLE`/`config.rs` person ids must match `server/config.py`'s `PEOPLE` list exactly.

Base URL: `http://<server-host>:8080` (configured in firmware `secrets.rs` as `SERVER_BASE_URL`, and in `server/config.py` as `HOST`/`PORT`).

## `POST /api/press`

Sent once per debounced button press.

Request body:
```json
{ "person": "alice" }
```

Response `200 OK`:
```json
{ "status": "ok", "person": "alice", "date": "2026-08-30" }
```

Response `400 Bad Request` (unknown or missing `person`):
```json
{ "status": "error", "message": "unknown person" }
```

## `POST /api/occupancy`

Sent once per confirmed (debounced) occupancy transition.

Request body:
```json
{ "occupied": true }
```

Response `200 OK`:
```json
{ "status": "ok", "occupied": true, "timestamp": "2026-08-30T21:14:03+09:00" }
```

Response `400 Bad Request` (missing/non-boolean `occupied`):
```json
{ "status": "error", "message": "occupied must be a boolean" }
```

The server does not deduplicate consecutive same-state occupancy POSTs — the firmware is responsible for debouncing before sending.

## `GET /api/status`

JSON snapshot used by the dashboard page and available for `curl`/testing.

Response `200 OK`:
```json
{
  "people": [
    { "id": "alice", "pressed_today": true, "last_pressed_at": "2026-08-30T19:02:11+09:00" },
    { "id": "bob", "pressed_today": false, "last_pressed_at": null }
  ],
  "occupied": false,
  "occupied_since": "2026-08-30T19:05:00+09:00"
}
```

## `GET /api/led-state`

Compact, embedded-friendly status used by the firmware's periodic NeoPixel sync (avoids JSON parsing on-device). One character per person, in `PEOPLE` order, `1` if `pressed_today` else `0`. `Content-Type: text/plain`.

Response `200 OK` body (for 5 people, e.g. alice and carol pressed today):
```
10100
```

The firmware polls this on an interval (see `firmware/src/status_poll.rs`) and reconciles all 5 NeoPixels to match — this is what turns the LEDs off again after the server's daily reset, and what restores correct LED state after a firmware reboot.

## `GET /`

Renders the HTML dashboard using the same data as `/api/status`.
