import pytest

import config
from app import create_app


@pytest.fixture
def client(tmp_path, monkeypatch):
    monkeypatch.setattr(config, "DB_PATH", tmp_path / "test.db")
    app = create_app()
    app.testing = True
    with app.test_client() as client:
        yield client


def test_press_unknown_person_is_rejected(client):
    resp = client.post("/api/press", json={"person": "zzz"})
    assert resp.status_code == 400


def test_press_marks_person_pressed_today(client):
    resp = client.post("/api/press", json={"person": "grandpa"})
    assert resp.status_code == 200
    assert resp.get_json()["person"] == "grandpa"

    status = client.get("/api/status").get_json()
    grandpa = next(p for p in status["people"] if p["id"] == "grandpa")
    assert grandpa["pressed_today"] is True

    grandma = next(p for p in status["people"] if p["id"] == "grandma")
    assert grandma["pressed_today"] is False


def test_press_is_idempotent_same_day(client):
    client.post("/api/press", json={"person": "grandpa"})
    resp = client.post("/api/press", json={"person": "grandpa"})
    assert resp.status_code == 200

    status = client.get("/api/status").get_json()
    assert sum(1 for p in status["people"] if p["id"] == "grandpa") == 1


def test_cancel_unknown_person_is_rejected(client):
    resp = client.post("/api/cancel", json={"person": "zzz"})
    assert resp.status_code == 400


def test_cancel_clears_a_press(client):
    client.post("/api/press", json={"person": "grandpa"})
    client.post("/api/press", json={"person": "father"})

    resp = client.post("/api/cancel", json={"person": "grandpa"})
    assert resp.status_code == 200
    assert resp.get_json()["person"] == "grandpa"

    status = client.get("/api/status").get_json()
    grandpa = next(p for p in status["people"] if p["id"] == "grandpa")
    assert grandpa["pressed_today"] is False
    assert grandpa["last_pressed_at"] is None
    father = next(p for p in status["people"] if p["id"] == "father")
    assert father["pressed_today"] is True


def test_cancel_is_idempotent_and_press_works_again_after(client):
    assert client.post("/api/cancel", json={"person": "grandpa"}).status_code == 200
    assert client.post("/api/cancel", json={"person": "grandpa"}).status_code == 200

    client.post("/api/press", json={"person": "grandpa"})
    client.post("/api/cancel", json={"person": "grandpa"})
    client.post("/api/press", json={"person": "grandpa"})
    status = client.get("/api/status").get_json()
    grandpa = next(p for p in status["people"] if p["id"] == "grandpa")
    assert grandpa["pressed_today"] is True


def test_occupancy_requires_boolean(client):
    resp = client.post("/api/occupancy", json={"occupied": "yes"})
    assert resp.status_code == 400


def test_occupancy_sequence_reflected_in_status(client):
    client.post("/api/occupancy", json={"occupied": True})
    status = client.get("/api/status").get_json()
    assert status["occupied"] is True

    client.post("/api/occupancy", json={"occupied": False})
    status = client.get("/api/status").get_json()
    assert status["occupied"] is False


def test_led_state_reflects_presses(client):
    resp = client.get("/api/led-state")
    assert resp.data.decode() == "0" * len(config.PEOPLE)

    client.post("/api/press", json={"person": "grandpa"})
    client.post("/api/press", json={"person": "father"})

    resp = client.get("/api/led-state")
    expected = "".join("1" if p in ("grandpa", "father") else "0" for p in config.PEOPLE)
    assert resp.data.decode() == expected


def test_index_page_lists_people(client):
    resp = client.get("/")
    assert resp.status_code == 200
    for person in config.PEOPLE:
        assert config.DISPLAY_NAMES[person] in resp.get_data(as_text=True)


def test_index_page_shows_display_names(client, monkeypatch):
    monkeypatch.setattr(
        config, "DISPLAY_NAMES", dict(config.DISPLAY_NAMES, grandpa="おじいちゃん")
    )
    client.post("/api/press", json={"person": "grandpa"})
    page = client.get("/").get_data(as_text=True)
    assert "おじいちゃん" in page
    assert "入浴済み ✓" in page
    assert "まだ" in page
    # The API keeps using ids.
    status = client.get("/api/status").get_json()
    assert status["people"][0]["id"] == "grandpa"


def test_index_page_shows_occupancy_in_japanese(client):
    assert "空いています" in client.get("/").get_data(as_text=True)
    client.post("/api/occupancy", json={"occupied": True})
    page = client.get("/").get_data(as_text=True)
    assert "入浴中" in page
    assert "から" in page
    client.post("/api/occupancy", json={"occupied": False})
    page = client.get("/").get_data(as_text=True)
    assert "空いています" in page
    assert "に出ました" in page


def test_display_names_fall_back_to_ids():
    names = config.load_display_names(
        {"BATH_MONITOR_NAME_GRANDPA": "おじいちゃん", "BATH_MONITOR_NAME_SON": "  "}
    )
    assert names["grandpa"] == "おじいちゃん"
    assert names["son"] == "son"
    assert names["father"] == "father"
    assert list(names) == config.PEOPLE


def _fix_now(monkeypatch, iso):
    import db
    from datetime import datetime

    monkeypatch.setattr(db, "_now", lambda: datetime.fromisoformat(iso))


def _grandpa_pressed(client):
    status = client.get("/api/status").get_json()
    return next(p for p in status["people"] if p["id"] == "grandpa")["pressed_today"]


def test_press_after_midnight_counts_for_previous_day(client, monkeypatch):
    _fix_now(monkeypatch, "2026-10-10T01:30:00+09:00")
    resp = client.post("/api/press", json={"person": "grandpa"})
    assert resp.get_json()["date"] == "2026-10-09"

    _fix_now(monkeypatch, "2026-10-10T04:59:59+09:00")
    assert _grandpa_pressed(client) is True


def test_presses_reset_at_day_start_hour(client, monkeypatch):
    _fix_now(monkeypatch, "2026-10-09T21:00:00+09:00")
    client.post("/api/press", json={"person": "grandpa"})

    _fix_now(monkeypatch, "2026-10-10T04:59:59+09:00")
    assert _grandpa_pressed(client) is True
    assert client.get("/api/led-state").get_data(as_text=True) == "10000"

    _fix_now(monkeypatch, "2026-10-10T05:00:00+09:00")
    assert _grandpa_pressed(client) is False
    assert client.get("/api/led-state").get_data(as_text=True) == "00000"


def test_short_time_shows_late_night_as_previous_day_past_24():
    from app import short_time

    assert short_time("2026-10-09T21:05:00+09:00") == "10/09 21:05"
    assert short_time("2026-10-10T01:30:00+09:00") == "10/09 25:30"
    assert short_time("2026-10-10T04:59:00+09:00") == "10/09 28:59"
    assert short_time("2026-10-10T05:00:00+09:00") == "10/10 05:00"
