"""The generated command reference in docs/cli/index.md must match the code.

Refresh it with: TS2_UPDATE_DOCS=1 uv run pytest tests/test_docs.py
"""

import os
import re
from pathlib import Path

import typer.main
from typer.cli import get_docs_for_click

from ts2.cli import app

PAGE = Path(__file__).parents[2] / "docs" / "cli" / "index.md"
BLOCK = re.compile(r"(<!-- cli-reference:start -->\n).*?(<!-- cli-reference:end -->)", re.S)


def render() -> str:
    cmd = typer.main.get_command(app)
    md = get_docs_for_click(obj=cmd, ctx=typer.Context(cmd), name="ts2")
    # Demote headings so the block nests under the page's "Command reference" section.
    return re.sub(r"^(#+) ", r"##\1 ", md, flags=re.M).strip() + "\n\n"


def test_cli_reference_is_current():
    page = PAGE.read_text()
    updated = BLOCK.sub(lambda m: m[1] + "\n" + render() + m[2], page)
    if os.environ.get("TS2_UPDATE_DOCS"):
        PAGE.write_text(updated)
    else:
        assert page == updated, "docs/cli/index.md is stale; run with TS2_UPDATE_DOCS=1"
