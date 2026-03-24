#!/usr/bin/env python3
from __future__ import annotations

import html
import re
import sys
import zipfile
from pathlib import Path


def render_inline(text: str) -> str:
    parts = re.split(r"(`[^`]+`)", text)
    out: list[str] = []
    for part in parts:
        if part.startswith("`") and part.endswith("`") and len(part) >= 2:
            out.append(f"<code>{html.escape(part[1:-1])}</code>")
        else:
            escaped = html.escape(part)
            escaped = re.sub(
                r"\[([^\]]+)\]\(([^)]+)\)",
                lambda m: f'<a href="{html.escape(m.group(2), quote=True)}">{html.escape(m.group(1))}</a>',
                escaped,
            )
            out.append(escaped)
    return "".join(out)


def parse_table_row(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def is_separator_row(cells: list[str]) -> bool:
    return all(cell and set(cell) <= {"-", ":"} for cell in cells)


def markdown_to_html(md: str) -> str:
    lines = md.splitlines()
    blocks: list[str] = []
    i = 0

    while i < len(lines):
        line = lines[i]

        if not line.strip():
            i += 1
            continue

        if line.startswith("### "):
            blocks.append(f"<h3>{render_inline(line[4:].strip())}</h3>")
            i += 1
            continue

        if line.startswith("## "):
            blocks.append(f"<h2>{render_inline(line[3:].strip())}</h2>")
            i += 1
            continue

        if line.startswith("# "):
            blocks.append(f"<h1>{render_inline(line[2:].strip())}</h1>")
            i += 1
            continue

        if line.startswith("- "):
            items: list[str] = []
            while i < len(lines) and lines[i].startswith("- "):
                items.append(f"<li>{render_inline(lines[i][2:].strip())}</li>")
                i += 1
            blocks.append("<ul>" + "".join(items) + "</ul>")
            continue

        if line.startswith("|"):
            table_lines: list[str] = []
            while i < len(lines) and lines[i].startswith("|"):
                table_lines.append(lines[i])
                i += 1

            rows = [parse_table_row(row) for row in table_lines]
            if len(rows) >= 2 and is_separator_row(rows[1]):
                header = rows[0]
                body = rows[2:]
            else:
                header = rows[0]
                body = rows[1:]

            thead = "".join(f"<th>{render_inline(cell)}</th>" for cell in header)
            tbody_rows = []
            for row in body:
                tbody_rows.append(
                    "<tr>" + "".join(f"<td>{render_inline(cell)}</td>" for cell in row) + "</tr>"
                )
            blocks.append(
                "<div class=\"table-wrap\"><table><thead><tr>"
                + thead
                + "</tr></thead><tbody>"
                + "".join(tbody_rows)
                + "</tbody></table></div>"
            )
            continue

        para: list[str] = []
        while i < len(lines) and lines[i].strip() and not lines[i].startswith(("#", "|", "- ")):
            para.append(lines[i].strip())
            i += 1
        blocks.append(f"<p>{render_inline(' '.join(para))}</p>")

    return "\n".join(blocks)


def build_page(body_html: str) -> str:
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>ayg Benchmark Report</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #f5f1e8;
      --panel: #fffdf8;
      --ink: #1a1816;
      --muted: #6b6257;
      --line: #d8ccbc;
      --accent: #0f766e;
      --accent-2: #134e4a;
      --shadow: rgba(43, 32, 20, 0.08);
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
      background:
        radial-gradient(circle at top left, rgba(15,118,110,0.12), transparent 28%),
        linear-gradient(180deg, #f8f4ec 0%, var(--bg) 100%);
      color: var(--ink);
    }}
    .shell {{
      max-width: 1160px;
      margin: 0 auto;
      padding: 48px 20px 72px;
    }}
    .hero {{
      background: linear-gradient(135deg, rgba(255,255,255,0.9), rgba(255,248,238,0.96));
      border: 1px solid rgba(216,204,188,0.9);
      border-radius: 24px;
      padding: 28px;
      box-shadow: 0 20px 60px var(--shadow);
      margin-bottom: 28px;
    }}
    .eyebrow {{
      margin: 0 0 8px;
      font-size: 0.9rem;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: var(--accent);
      font-weight: 700;
    }}
    h1, h2, h3 {{
      font-family: "Avenir Next Condensed", "Franklin Gothic Medium", "Arial Narrow", sans-serif;
      letter-spacing: 0.01em;
      margin: 0 0 14px;
      line-height: 1.05;
    }}
    h1 {{ font-size: clamp(2.3rem, 4vw, 4rem); }}
    h2 {{
      font-size: clamp(1.6rem, 2.4vw, 2.2rem);
      margin-top: 34px;
      padding-top: 18px;
      border-top: 1px solid var(--line);
    }}
    h3 {{
      font-size: 1.2rem;
      margin-top: 24px;
    }}
    p, li {{
      font-size: 1.02rem;
      line-height: 1.7;
      color: var(--ink);
    }}
    .actions {{
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      margin-top: 18px;
    }}
    .btn {{
      display: inline-flex;
      align-items: center;
      justify-content: center;
      padding: 12px 16px;
      border-radius: 999px;
      background: var(--accent);
      color: white;
      text-decoration: none;
      font-weight: 700;
      box-shadow: 0 10px 24px rgba(15,118,110,0.22);
    }}
    .btn.secondary {{
      background: transparent;
      color: var(--accent-2);
      border: 1px solid rgba(19,78,74,0.22);
      box-shadow: none;
    }}
    .card {{
      background: var(--panel);
      border: 1px solid rgba(216,204,188,0.8);
      border-radius: 20px;
      padding: 28px;
      box-shadow: 0 14px 36px var(--shadow);
    }}
    .table-wrap {{
      overflow-x: auto;
      margin: 18px 0 24px;
      border: 1px solid rgba(216,204,188,0.75);
      border-radius: 18px;
      background: #fffdfa;
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
      min-width: 720px;
    }}
    th, td {{
      padding: 12px 14px;
      text-align: left;
      border-bottom: 1px solid rgba(216,204,188,0.7);
      vertical-align: top;
    }}
    th {{
      background: #f4ece0;
      color: var(--accent-2);
      font-family: "Avenir Next Condensed", "Franklin Gothic Medium", "Arial Narrow", sans-serif;
      font-size: 0.95rem;
      letter-spacing: 0.04em;
      text-transform: uppercase;
    }}
    tr:last-child td {{ border-bottom: 0; }}
    code {{
      font-family: "SFMono-Regular", "Menlo", "Consolas", monospace;
      font-size: 0.92em;
      background: rgba(19,78,74,0.08);
      padding: 0.16em 0.4em;
      border-radius: 0.4em;
    }}
    ul {{
      padding-left: 1.2rem;
      margin: 0 0 18px;
    }}
    a {{
      color: var(--accent-2);
    }}
    footer {{
      margin-top: 28px;
      color: var(--muted);
      font-size: 0.95rem;
    }}
    @media (max-width: 700px) {{
      .shell {{ padding: 24px 14px 48px; }}
      .hero, .card {{ padding: 18px; border-radius: 18px; }}
      .actions {{ flex-direction: column; }}
      .btn {{ width: 100%; }}
    }}
  </style>
</head>
<body>
  <main class="shell">
    <section class="hero">
      <p class="eyebrow">Public Benchmark Report</p>
      <h1>ayg Benchmarks</h1>
      <p>Full local and GitHub Actions benchmark output, formatted for humans and packaged for download.</p>
      <div class="actions">
        <a class="btn" href="./benchmark-report.zip">Download ZIP</a>
        <a class="btn secondary" href="./results.md">Raw Markdown</a>
      </div>
    </section>
    <section class="card">
      {body_html}
    </section>
    <footer>
      This page is generated from the benchmark workflow output.
    </footer>
  </main>
</body>
</html>
"""


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: render-benchmark-site.py <results.md> <output-dir>", file=sys.stderr)
        return 2

    src = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)

    md_text = src.read_text()
    (out_dir / "results.md").write_text(md_text)

    body_html = markdown_to_html(md_text)
    (out_dir / "index.html").write_text(build_page(body_html))

    archive_path = out_dir / "benchmark-report.zip"
    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.write(out_dir / "index.html", arcname="index.html")
        zf.write(out_dir / "results.md", arcname="results.md")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
