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
    resp = client.post("/api/press", json={"person": "alice"})
    assert resp.status_code == 200
    assert resp.get_json()["person"] == "alice"

    status = client.get("/api/status").get_json()
    alice = next(p for p in status["people"] if p["id"] == "alice")
    assert alice["pressed_today"] is True

    bob = next(p for p in status["people"] if p["id"] == "bob")
    assert bob["pressed_today"] is False


def test_press_is_idempotent_same_day(client):
    client.post("/api/press", json={"person": "alice"})
    resp = client.post("/api/press", json={"person": "alice"})
    assert resp.status_code == 200

    status = client.get("/api/status").get_json()
    assert sum(1 for p in status["people"] if p["id"] == "alice") == 1


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

    client.post("/api/press", json={"person": "alice"})
    client.post("/api/press", json={"person": "carol"})

    resp = client.get("/api/led-state")
    expected = "".join("1" if p in ("alice", "carol") else "0" for p in config.PEOPLE)
    assert resp.data.decode() == expected


def test_index_page_lists_people(client):
    resp = client.get("/")
    assert resp.status_code == 200
    for person in config.PEOPLE:
        assert person.encode() in resp.data
