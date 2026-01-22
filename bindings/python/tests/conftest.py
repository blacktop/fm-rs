"""Pytest configuration for fm tests."""

import pytest


def pytest_configure(config):
    """Configure custom markers."""
    config.addinivalue_line(
        "markers",
        "integration: mark test as requiring Apple Intelligence (deselect with '-m \"not integration\"')",
    )
