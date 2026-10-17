import os
from pathlib import Path

# Must match firmware/src/config.rs PEOPLE order/ids exactly.
PEOPLE = ["grandpa", "grandma", "father", "mother", "son"]


def load_display_names(environ) -> dict:
    """Dashboard-only display name per person, from BATH_MONITOR_NAME_<ID>
    environment variables (set with Environment= in the systemd unit, so
    family names stay out of the repo). A person whose variable is unset or
    empty is shown by their id. Ids themselves never change: they are the
    firmware contract and the DB key."""
    names = {}
    for person in PEOPLE:
        name = environ.get("BATH_MONITOR_NAME_" + person.upper(), "").strip()
        names[person] = name or person
    return names


DISPLAY_NAMES = load_display_names(os.environ)

# Local hour at which presses reset for the new day, so a day runs from 05:00
# to "29:00" (05:00 the next morning) under one date and late-night baths
# still count for the previous evening. The dashboard shows 00:00-04:59 as
# 24:00-28:59 of the previous date.
DAY_START_HOUR = 5

BASE_DIR = Path(__file__).resolve().parent
DB_PATH = BASE_DIR / "data" / "bath_monitor.db"
SCHEMA_PATH = BASE_DIR / "schema.sql"

HOST = "0.0.0.0"
PORT = 8080
