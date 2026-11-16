#!/usr/bin/env python3
"""Побудова графіків чисельності організмів за ітераціями для всіх прогонів у директорії з відображенням метрик на зображенні."""

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
    "plants": ("Рослини (водорості)", "#2ca02c"),
    "herbivores": ("Травоїдні", "#1f77b4"),
    "predators": ("Хижаки", "#d62728"),
}


def resolve_csv_paths(path: Path) -> list[Path]:
    if path.is_dir():
        candidates = sorted(path.glob("run_*.csv"))
        if not candidates:
            raise FileNotFoundError(f"У директорії не знайдено файлів 'run_*.csv': {path}")
        return candidates
    if not path.is_file():
        raise FileNotFoundError(f"CSV-файл або директорію не знайдено: {path}")
    return [path]


def load_run_csv(csv_path: Path) -> list[dict[str, str]]:
    with csv_path.open("r", encoding="utf-8", newline="") as f:
        reader = csv.DictReader(f)
        rows = list(reader)
    if not rows:
        raise ValueError(f"CSV-файл прогону порожній: {csv_path}")
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
        f"CSV '{csv_path}' не містить стовпців max_age/reproductions, а відповідний запис у summary.csv не знайдено."
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
                raise ValueError(f"Невідомий тип організмів '{token}'. Допустимі значення: {valid}")
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
                f"Попередження: стовпець '{col}' відсутній у {csv_path.name}; пропускаємо.",
                file=sys.stderr,
            )
            continue
        values = [int(r[col]) for r in rows]
        label, color = ENTITY_LABELS[col]
        ax.plot(ticks, values, label=label, color=color, linewidth=1.8)

    ax.set_title(
        "Чисельність популяції за ітераціями",
        fontsize=12,
        fontweight="bold",
    )
    ax.set_xlabel("Ітерація (крок)")
    ax.set_ylabel("Кількість особин")
    ax.grid(True, linestyle="--", alpha=0.4)
    ax.legend(loc="upper right")

    metrics_banner = (
        f"Макс. вік особин (кроків)   —   Травоїдні: {metrics['max_age_herbivores']}   |   "
        f"Хижаки: {metrics['max_age_predators']}\n"
        f"Кількість розмножень   —   Рослини: {metrics['reproductions_plants']}   |   "
        f"Травоїдні: {metrics['reproductions_herbivores']}   |   "
        f"Хижаки: {metrics['reproductions_predators']}"
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
        f"[{csv_path.name}] кроки={ticks[0]}..{ticks[-1]} ({len(rows)} записів) | "
        f"макс_вік=(Т:{metrics['max_age_herbivores']}, Х:{metrics['max_age_predators']}) | "
        f"розмноження=(Р:{metrics['reproductions_plants']}, Т:{metrics['reproductions_herbivores']}, Х:{metrics['reproductions_predators']}) "
        f"-> {output_path}"
    )
    return output_path


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Побудова графіків кількості організмів заданого типу (plant/herbivore/predator) "
            "за ітераціями для всіх прогонів у директорії (або одного CSV-файлу) зі збереженням "
            "зображення поруч із файлом прогону та відображенням макс. віку й кількості розмножень."
        )
    )
    parser.add_argument(
        "input",
        nargs="?",
        type=Path,
        default=Path("results/"),
        help="Шлях до директорії з файлами run_*.csv або до окремого файлу run_*.csv (за замовчуванням: ./results/)",
    )
    parser.add_argument(
        "-t",
        "--type",
        dest="entity_types",
        action="append",
        default=[],
        help=(
            "Типи організмів для побудови: plant, herbivore, predator або all "
            "(можна вказувати кілька разів або через кому; за замовчуванням: all)"
        ),
    )
    parser.add_argument(
        "-f",
        "--format",
        default="png",
        help="Формат/розширення файлу зображення поруч із кожним CSV прогону (за замовчуванням: png)",
    )
    args = parser.parse_args()

    try:
        csv_paths = resolve_csv_paths(args.input)
        selected_columns = parse_entity_types(args.entity_types)
    except Exception as exc:
        print(f"Помилка: {exc}", file=sys.stderr)
        return 1

    base_dir = args.input if args.input.is_dir() else args.input.parent
    summary_index = load_summary_index(base_dir)

    saved_count = 0
    for csv_path in csv_paths:
        try:
            plot_single_run(csv_path, selected_columns, summary_index, args.format)
            saved_count += 1
        except Exception as exc:
            print(f"Помилка побудови графіка для '{csv_path}': {exc}", file=sys.stderr)
            return 1

    print(f"\nЗгенеровано зображень графіків: {saved_count} (у {base_dir})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
