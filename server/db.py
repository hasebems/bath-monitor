import sqlite3
from datetime import datetime
from pathlib import Path

import config
from models import PersonStatus, Status


def _connect() -> sqlite3.Connection:
    config.DB_PATH.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(config.DB_PATH)
    conn.row_factory = sqlite3.Row
    return conn


def init_db() -> None:
    with _connect() as conn:
        conn.executescript(Path(config.SCHEMA_PATH).read_text())
        conn.executemany(
            "INSERT OR IGNORE INTO press_status (person, last_pressed_at, last_pressed_date) "
            "VALUES (?, NULL, NULL)",
            [(person,) for person in config.PEOPLE],
        )


def get_status() -> Status:
    today = datetime.now().date().isoformat()
    with _connect() as conn:
        rows = conn.execute(
            "SELECT person, last_pressed_at, last_pressed_date FROM press_status"
        ).fetchall()
        occ_row = conn.execute(
            "SELECT occupied, changed_at FROM occupancy_log ORDER BY id DESC LIMIT 1"
        ).fetchone()

    by_person = {row["person"]: row for row in rows}
    people = [
        PersonStatus(
            id=person,
            pressed_today=(by_person[person]["last_pressed_date"] == today)
            if person in by_person
            else False,
            last_pressed_at=by_person[person]["last_pressed_at"] if person in by_person else None,
        )
        for person in config.PEOPLE
    ]

    occupied = bool(occ_row["occupied"]) if occ_row is not None else False
    occupied_since = occ_row["changed_at"] if occ_row is not None else None

    return Status(people=people, occupied=occupied, occupied_since=occupied_since)


def get_led_state() -> str:
    status = get_status()
    return "".join("1" if person.pressed_today else "0" for person in status.people)


def record_press(person: str) -> PersonStatus:
    now = datetime.now().astimezone()
    now_iso = now.isoformat()
    today = now.date().isoformat()
    with _connect() as conn:
        conn.execute(
            "UPDATE press_status SET last_pressed_at = ?, last_pressed_date = ? WHERE person = ?",
            (now_iso, today, person),
        )
    return PersonStatus(id=person, pressed_today=True, last_pressed_at=now_iso)


def record_occupancy(occupied: bool) -> tuple[bool, str]:
    now_iso = datetime.now().astimezone().isoformat()
    with _connect() as conn:
        conn.execute(
            "INSERT INTO occupancy_log (occupied, changed_at) VALUES (?, ?)",
            (1 if occupied else 0, now_iso),
        )
    return occupied, now_iso
