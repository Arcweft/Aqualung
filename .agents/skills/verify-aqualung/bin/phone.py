#!/usr/bin/env python3
"""Delegate to docker/phone.py so documented skill paths keep working."""
from __future__ import annotations

import runpy
from pathlib import Path

root = Path(__file__).resolve().parents[4]
runpy.run_path(str(root / "docker" / "phone.py"), run_name="__main__")
