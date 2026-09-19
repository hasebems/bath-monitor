from __future__ import annotations

from dataclasses import asdict
from datetime import datetime

from flask import Flask, jsonify, render_template, request

import config
import db


def short_time(iso_str: str | None) -> str:
    """Compact display format for a timestamp, e.g. '09/01 15:49' — the
    raw ISO8601+offset string is too long for the mobile-width dashboard."""
    if not iso_str:
        return ""
    try:
        dt = datetime.fromisoformat(iso_str)
    except ValueError:
        return iso_str
    return dt.strftime("%m/%d %H:%M")


def create_app() -> Flask:
    app = Flask(__name__)
    app.jinja_env.filters["short_time"] = short_time
    db.init_db()

    @app.post("/api/press")
    def api_press():
        body = request.get_json(silent=True) or {}
        person = body.get("person")
        if person not in config.PEOPLE:
            return jsonify(status="error", message="unknown person"), 400
        status = db.record_press(person)
        return jsonify(status="ok", person=status.id, date=status.last_pressed_at[:10])

    @app.post("/api/cancel")
    def api_cancel():
        body = request.get_json(silent=True) or {}
        person = body.get("person")
        if person not in config.PEOPLE:
            return jsonify(status="error", message="unknown person"), 400
        status = db.cancel_press(person)
        return jsonify(status="ok", person=status.id)

    @app.post("/api/occupancy")
    def api_occupancy():
        body = request.get_json(silent=True) or {}
        occupied = body.get("occupied")
        if not isinstance(occupied, bool):
            return jsonify(status="error", message="occupied must be a boolean"), 400
        occupied, timestamp = db.record_occupancy(occupied)
        return jsonify(status="ok", occupied=occupied, timestamp=timestamp)

    @app.get("/api/status")
    def api_status():
        return jsonify(asdict(db.get_status()))

    @app.get("/api/led-state")
    def api_led_state():
        return db.get_led_state(), 200, {"Content-Type": "text/plain"}

    @app.get("/")
    def index():
        status = db.get_status()
        return render_template("status.html", status=status)

    return app


if __name__ == "__main__":
    create_app().run(host=config.HOST, port=config.PORT, threaded=True)
