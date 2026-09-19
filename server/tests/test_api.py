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
        assert person.encode() in resp.data
