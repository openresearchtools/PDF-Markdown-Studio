#!/usr/bin/env python3
"""Generate a synthetic demo PDF with text, a table, and a chart.

Output:
  demo/demo.pdf
"""

from __future__ import annotations

import io
from pathlib import Path

import matplotlib

# Force non-interactive backend for headless environments.
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from reportlab.lib import colors
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle, getSampleStyleSheet
from reportlab.lib.units import mm
from reportlab.platypus import Image, Paragraph, SimpleDocTemplate, Spacer, Table, TableStyle


def build_chart() -> io.BytesIO:
    """Return a PNG chart as an in-memory buffer."""
    batches = ["B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8"]
    texture_score = [7.2, 7.8, 8.3, 8.1, 8.7, 8.6, 9.0, 8.8]
    fluffiness_index = [6.9, 7.2, 7.9, 7.7, 8.1, 8.3, 8.5, 8.4]

    fig, ax = plt.subplots(figsize=(8.4, 3.6), dpi=180)
    ax.plot(batches, texture_score, marker="o", linewidth=2.0, label="Texture score")
    ax.plot(batches, fluffiness_index, marker="s", linewidth=2.0, label="Fluffiness index")
    ax.set_title("Pancake Trial Scores by Batch", fontsize=12, pad=10)
    ax.set_xlabel("Batch")
    ax.set_ylabel("Score (0-10)")
    ax.set_ylim(6.0, 9.5)
    ax.grid(True, linestyle="--", alpha=0.35)
    ax.legend(loc="lower right", frameon=True)
    fig.tight_layout()

    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    plt.close(fig)
    buf.seek(0)
    return buf


def build_demo_pdf(output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    doc = SimpleDocTemplate(
        str(output_path),
        pagesize=A4,
        leftMargin=16 * mm,
        rightMargin=16 * mm,
        topMargin=16 * mm,
        bottomMargin=16 * mm,
        title="Demo Pancake Study",
        author="PDF Markdown Studio Demo Generator",
    )

    styles = getSampleStyleSheet()
    h1 = styles["Heading1"]
    h2 = styles["Heading2"]
    body = ParagraphStyle(
        "BodyCompact",
        parent=styles["BodyText"],
        fontSize=10.5,
        leading=14,
        spaceAfter=6,
    )
    mono_note = ParagraphStyle(
        "MonoNote",
        parent=styles["BodyText"],
        fontName="Courier",
        fontSize=9,
        leading=12,
        textColor=colors.HexColor("#444444"),
    )

    story = []
    story.append(Paragraph("Demo Report: Pancake Optimization Trials", h1))
    story.append(
        Paragraph(
            "This is fully synthetic content for screenshots and testing. "
            "It contains a moderately complex table and a simple chart designed to be OCR/VLM friendly.",
            body,
        )
    )
    story.append(
        Paragraph(
            "Scenario: a fictional kitchen team tested eight pancake batches while varying flour blend, "
            "rest time, pan temperature, and syrup viscosity. Metrics were scored on a 0-10 scale.",
            body,
        )
    )

    story.append(Spacer(1, 5))
    story.append(Paragraph("Table 1. Batch Parameters and Outcomes", h2))

    table_data = [
        [
            "Batch",
            "Flour Blend",
            "Milk Type",
            "Rest (min)",
            "Pan Temp (C)",
            "Flip Time (s)",
            "Syrup Viscosity (cP)",
            "Texture",
            "Fluffiness",
        ],
        ["B1", "70% wheat / 30% oat", "Whole", "5", "178", "44", "980", "7.2", "6.9"],
        ["B2", "70% wheat / 30% oat", "Whole", "10", "180", "42", "980", "7.8", "7.2"],
        ["B3", "60% wheat / 40% oat", "Whole", "12", "182", "41", "960", "8.3", "7.9"],
        ["B4", "60% wheat / 40% oat", "Skim", "12", "184", "40", "940", "8.1", "7.7"],
        ["B5", "55% wheat / 45% oat", "Skim", "15", "186", "39", "920", "8.7", "8.1"],
        ["B6", "55% wheat / 45% oat", "Almond", "15", "186", "38", "900", "8.6", "8.3"],
        ["B7", "50% wheat / 50% oat", "Almond", "18", "188", "37", "890", "9.0", "8.5"],
        ["B8", "50% wheat / 50% oat", "Almond", "20", "188", "36", "870", "8.8", "8.4"],
    ]

    col_widths = [
        17 * mm,
        41 * mm,
        20 * mm,
        17 * mm,
        23 * mm,
        21 * mm,
        31 * mm,
        16 * mm,
        20 * mm,
    ]

    table = Table(table_data, colWidths=col_widths, repeatRows=1)
    table.setStyle(
        TableStyle(
            [
                ("BACKGROUND", (0, 0), (-1, 0), colors.HexColor("#2D5A80")),
                ("TEXTCOLOR", (0, 0), (-1, 0), colors.white),
                ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
                ("FONTNAME", (0, 1), (-1, -1), "Helvetica"),
                ("FONTSIZE", (0, 0), (-1, -1), 8.7),
                ("ALIGN", (0, 0), (-1, -1), "CENTER"),
                ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
                ("ROWBACKGROUNDS", (0, 1), (-1, -1), [colors.HexColor("#F7FAFC"), colors.white]),
                ("GRID", (0, 0), (-1, -1), 0.7, colors.HexColor("#7A8A99")),
                ("LEFTPADDING", (0, 0), (-1, -1), 4),
                ("RIGHTPADDING", (0, 0), (-1, -1), 4),
                ("TOPPADDING", (0, 0), (-1, -1), 5),
                ("BOTTOMPADDING", (0, 0), (-1, -1), 5),
            ]
        )
    )
    story.append(table)

    story.append(Spacer(1, 10))
    story.append(Paragraph("Figure 1. Score Trends Across Batches", h2))
    chart_buf = build_chart()
    story.append(Image(chart_buf, width=172 * mm, height=72 * mm))

    story.append(Spacer(1, 7))
    story.append(
        Paragraph(
            "Interpretation: score trends improve as rest time increases and syrup viscosity decreases. "
            "In this fictional setup, batch B7 is the top performer.",
            body,
        )
    )
    story.append(
        Paragraph(
            "Data note: this report is intentionally fabricated for demo purposes only.",
            mono_note,
        )
    )

    doc.build(story)


def main() -> None:
    repo_root = Path(__file__).resolve().parents[1]
    out = repo_root / "demo" / "demo.pdf"
    build_demo_pdf(out)
    print(out)


if __name__ == "__main__":
    main()
