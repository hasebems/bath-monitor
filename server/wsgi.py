"""Production entry point (used by the systemd service). Run directly with
`python app.py` instead for local development with the Flask dev server."""

from waitress import serve

import config
from app import create_app

if __name__ == "__main__":
    app = create_app()
    serve(app, host=config.HOST, port=config.PORT)
