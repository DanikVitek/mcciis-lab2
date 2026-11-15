#!/usr/bin/env python3
"""Plot entity population over iterations for all runs in a directory and display run metrics on each image."""

from __future__ import annotations

import argparse
import csv
from pathlib import Path
import sys
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

ENTITY_COLUMN_MAP = {
    "plant": "plants",
    "plants": "plants",
    "herbivore": "herbivores",
    "herbivores": "herbivores",
    "predator": "predators",
    "predators": "predators",
}

ENTITY_LABELS = {
    "plants": ("Plant (Algae)", "#2ca02c"),
    "herbivores": ("Herbivore", "#1f77b4"),
    "predators": ("Predator", "#d62728"),
}


def resolve_csv_paths(path: Path) -> list[Path]:
    if path.is_dir():
        candidates = sorted(path.glob("run_*.csv"))
        if not candidates:
            raise FileNotFoundError(f"No 'run_*.csv' files found in directory: {path}")
        return candidates
    if not path.is_file():
        raise FileNotFoundError(f"CSV file or directory not found: {path}")
    return [path]


def load_run_csv(csv_path: Path) -> list[dict[str, str]]:
    with csv_path.open("r", encoding="utf-8", newline="") as f:
        reader = csv.DictReader(f)
        rows = list(reader)
    if not rows:
        raise ValueError(f"Run CSV file is empty: {csv_path}")
    return rows


def load_summary_index(directory: Path) -> dict[str, dict[str, str]]:
    summary_path = directory / "summary.csv"
    if not summary_path.is_file():
        return {}
    with summary_path.open("r", encoding="utf-8", newline="") as f:
        return {
            row["history_file"]: row
            for row in csv.DictReader(f)
            if "history_file" in row
        }


def extract_summary_metrics(
    rows: list[dict[str, str]],
    csv_path: Path,
    summary_index: dict[str, dict[str, str]],
) -> dict[str, int]:
    last = rows[-1]
    if "max_age_herbivores" in last and "reproductions_plants" in last:
        return {
            "max_age_herbivores": max(int(r["max_age_herbivores"]) for r in rows),
            "max_age_predators": max(int(r["max_age_predators"]) for r in rows),
            "reproductions_plants": max(int(r["reproductions_plants"]) for r in rows),
            "reproductions_herbivores": max(int(r["reproductions_herbivores"]) for r in rows),
            "reproductions_predators": max(int(r["reproductions_predators"]) for r in rows),
        }

    if csv_path.name in summary_index:
        row = summary_index[csv_path.name]
        return {
            "max_age_herbivores": int(row.get("max_age_herbivores", 0)),
            "max_age_predators": int(row.get("max_age_predators", 0)),
            "reproductions_plants": int(row.get("reproductions_plants", 0)),
            "reproductions_herbivores": int(row.get("reproductions_herbivores", 0)),
            "reproductions_predators": int(row.get("reproductions_predators", 0)),
        }

    raise KeyError(
        f"CSV '{csv_path}' does not contain max_age/reproductions columns and no matching summary.csv entry was found."
    )


def parse_entity_types(raw_types: list[str]) -> list[str]:
    if not raw_types or "all" in [t.lower() for t in raw_types]:
        return ["plants", "herbivores", "predators"]

    selected: list[str] = []
    for item in raw_types:
        for token in item.split(","):
            key = token.strip().lower()
            if not key:
                continue
            if key == "all":
                return ["plants", "herbivores", "predators"]
            if key not in ENTITY_COLUMN_MAP:
                valid = ", ".join(["plant", "herbivore", "predator", "all"])
                raise ValueError(f"Unknown entity type '{token}'. Choose from: {valid}")
            col = ENTITY_COLUMN_MAP[key]
            if col not in selected:
                selected.append(col)
    return selected


def plot_single_run(
    csv_path: Path,
    selected_columns: list[str],
    summary_index: dict[str, dict[str, str]],
    image_ext: str,
) -> Path:
    rows = load_run_csv(csv_path)
    metrics = extract_summary_metrics(rows, csv_path, summary_index)
    ticks = [int(r["tick"]) for r in rows]

    fig, ax = plt.subplots(figsize=(10.5, 6.2))
    for col in selected_columns:
        if col not in rows[0]:
            print(
                f"Warning: column '{col}' is not present in {csv_path.name}; skipping.",
                file=sys.stderr,
            )
            continue
        values = [int(r[col]) for r in rows]
        label, color = ENTITY_LABELS[col]
        ax.plot(ticks, values, label=label, color=color, linewidth=1.8)

    ax.set_title(f"Entity Population by Iteration ({csv_path.stem})", fontsize=12, fontweight="bold")
    ax.set_xlabel("Iteration (Tick)")
    ax.set_ylabel("Entity Count")
    ax.grid(True, linestyle="--", alpha=0.4)
    ax.legend(loc="upper right")

    metrics_banner = (
        f"Max Character Age (ticks)   —   Herbivore: {metrics['max_age_herbivores']}   |   "
        f"Predator: {metrics['max_age_predators']}\n"
        f"Reproductions Performed   —   Plant: {metrics['reproductions_plants']}   |   "
        f"Herbivore: {metrics['reproductions_herbivores']}   |   "
        f"Predator: {metrics['reproductions_predators']}"
    )
    fig.text(
        0.5,
        0.015,
        metrics_banner,
        ha="center",
        va="bottom",
        fontsize=10,
        fontweight="medium",
        bbox={
            "boxstyle": "round,pad=0.5",
            "facecolor": "#f0f4f8",
            "edgecolor": "#9aa5b1",
            "linewidth": 1.2,
        },
    )
    fig.tight_layout(rect=(0.0, 0.09, 1.0, 1.0))

    ext = image_ext if image_ext.startswith(".") else f".{image_ext}"
    output_path = csv_path.with_suffix(ext)
    fig.savefig(output_path, dpi=150)
    plt.close(fig)

    print(
        f"[{csv_path.name}] ticks={ticks[0]}..{ticks[-1]} ({len(rows)} samples) | "
        f"max_age=(H:{metrics['max_age_herbivores']}, P:{metrics['max_age_predators']}) | "
        f"reproductions=(Pl:{metrics['reproductions_plants']}, H:{metrics['reproductions_herbivores']}, P:{metrics['reproductions_predators']}) "
        f"-> {output_path}"
    )
    return output_path


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Plot the amount of entities of a given type (plant/herbivore/predator) across "
            "iterations for all runs in a directory (or a single CSV file), saving each plot "
            "next to its run CSV with max age and reproduction counts displayed on the image."
        )
    )
    parser.add_argument(
        "input",
        nargs="?",
        type=Path,
        default=Path("protocol_results"),
        help="Path to a results directory containing run_*.csv files or a single run_*.csv file (default: ./protocol_results)",
    )
    parser.add_argument(
        "-t",
        "--type",
        dest="entity_types",
        action="append",
        default=[],
        help=(
            "Entity type(s) to plot: plant, herbivore, predator, or all "
            "(can be passed multiple times or comma-separated; default: all)"
        ),
    )
    parser.add_argument(
        "-f",
        "--format",
        default="png",
        help="Image file extension/format to save next to each run CSV (default: png)",
    )
    args = parser.parse_args()

    try:
        csv_paths = resolve_csv_paths(args.input)
        selected_columns = parse_entity_types(args.entity_types)
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1

    base_dir = args.input if args.input.is_dir() else args.input.parent
    summary_index = load_summary_index(base_dir)

    saved_count = 0
    for csv_path in csv_paths:
        try:
            plot_single_run(csv_path, selected_columns, summary_index, args.format)
            saved_count += 1
        except Exception as exc:
            print(f"Error plotting '{csv_path}': {exc}", file=sys.stderr)
            return 1

    print(f"\nGenerated {saved_count} plot image(s) in {base_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
