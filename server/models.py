from __future__ import annotations

from dataclasses import dataclass


@dataclass
class PersonStatus:
    id: str
    pressed_today: bool
    last_pressed_at: str | None


@dataclass
class Status:
    people: list[PersonStatus]
    occupied: bool
    occupied_since: str | None
