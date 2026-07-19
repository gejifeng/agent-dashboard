# Archived Python dashboard prototype

This directory contains the v0 proof of concept that preceded the Rust/Tauri application.
It is retained for architecture history and lightweight API experiments; it is not part of
the production desktop runtime.

The prototype includes:

- `server.py`: a minimal HTTP status server;
- `status_store.py`: JSON-backed status storage;
- `report_cli.py`: a command-line reporter;
- `cli_wrapper.py`: an experimental Anthropic-based CLI wrapper.

Run commands from the repository root so `server.py` can serve `frontend/index.html`:

```powershell
python prototypes/python-dashboard/server.py
python prototypes/python-dashboard/report_cli.py --help
```

`cli_wrapper.py` requires the optional `anthropic` Python package and an
`ANTHROPIC_API_KEY`. Do not place credentials in this directory or commit them.
