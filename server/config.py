from pathlib import Path

# Must match firmware/src/config.rs PEOPLE order/ids exactly.
PEOPLE = ["grandpa", "grandma", "father", "mother", "tamaki"]

BASE_DIR = Path(__file__).resolve().parent
DB_PATH = BASE_DIR / "data" / "bath_monitor.db"
SCHEMA_PATH = BASE_DIR / "schema.sql"

HOST = "0.0.0.0"
PORT = 8080
