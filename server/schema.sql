CREATE TABLE IF NOT EXISTS press_status (
    person TEXT PRIMARY KEY,
    last_pressed_at TEXT,
    last_pressed_date TEXT
);

CREATE TABLE IF NOT EXISTS occupancy_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occupied INTEGER NOT NULL CHECK (occupied IN (0, 1)),
    changed_at TEXT NOT NULL
);
