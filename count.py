#!/usr/bin/env python3
"""AviQtl project source code line counter"""

import sys
from pathlib import Path
from collections import defaultdict

# Target file extensions and their display names
TARGET_EXTENSIONS: dict[str, str] = {
    ".cpp":  "C++  Source",
    ".hpp":  "C++  Header",
    ".qml":  "QML",
    ".js":   "JavaScript",
    ".py":   "Python",
    ".glsl": "GLSL Shader",
    ".frag": "GLSL Fragment",
    ".vert": "GLSL Vertex",
}

# Also include CMakeLists.txt as an individual target
CMAKE_FILENAME = "CMakeLists.txt"

# Directories excluded from scanning
EXCLUDE_DIRS: frozenset[str] = frozenset({
    ".git", ".build_tmp", "build", "dist",
    ".cache", "__pycache__",
})


def is_excluded(path: Path) -> bool:
    """Check if any part of the path is in the exclude directories"""
    return any(part in EXCLUDE_DIRS for part in path.parts)


def count_lines(path: Path) -> tuple[int, int, int]:
    """
    Return line counts for a file.
    Returns: (total lines, code lines, blank lines)
    """
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return 0, 0, 0

    total = len(lines)
    blank = sum(1 for line in lines if not line.strip())
    code = total - blank
    return total, code, blank


def collect_files(root: Path) -> list[Path]:
    """Recursively collect target files from root directory"""
    result: list[Path] = []
    for path in root.rglob("*"):
        if path.is_dir():
            continue
        rel = path.relative_to(root)
        if is_excluded(rel):
            continue
        if path.suffix in TARGET_EXTENSIONS:
            result.append(path)
        elif path.name == CMAKE_FILENAME:
            result.append(path)
    return sorted(result)


def format_table_row(label: str, files: int, total: int, code: int, blank: int) -> str:
    return f"  {label:<22} {files:>5} files  {total:>7} lines  {code:>7} lines  {blank:>7} lines"


def main() -> None:
    root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).parent.resolve()

    print(f"\n{'=' * 70}")
    print(f"  AviQtl Project Source Code Line Count Report")
    print(f"  Scan target: {root}")
    print(f"{'=' * 70}")

    files = collect_files(root)

    # Aggregate by extension
    stats: dict[str, dict] = defaultdict(lambda: {"files": 0, "total": 0, "code": 0, "blank": 0})

    for path in files:
        key = path.name if path.name == CMAKE_FILENAME else path.suffix
        total, code, blank = count_lines(path)
        stats[key]["files"] += 1
        stats[key]["total"] += total
        stats[key]["code"]  += code
        stats[key]["blank"] += blank

    # Display order
    display_order = list(TARGET_EXTENSIONS.keys()) + [CMAKE_FILENAME]

    print(f"\n  {'Type':<22} {'Files':>10}  {'Total':>10}  {'Code':>10}  {'Blank':>10}")
    print(f"  {'-' * 66}")

    grand_files = grand_total = grand_code = grand_blank = 0

    for key in display_order:
        if key not in stats:
            continue
        s = stats[key]
        label = TARGET_EXTENSIONS.get(key, key)
        print(format_table_row(label, s["files"], s["total"], s["code"], s["blank"]))
        grand_files += s["files"]
        grand_total += s["total"]
        grand_code  += s["code"]
        grand_blank += s["blank"]

    print(f"  {'-' * 66}")
    print(format_table_row("Total", grand_files, grand_total, grand_code, grand_blank))
    print(f"{'=' * 70}\n")

    # Display top 10 files by code line count
    file_ranks: list[tuple[int, int, Path]] = []
    for path in files:
        total, code, blank = count_lines(path)
        file_ranks.append((code, total, path))

    file_ranks.sort(reverse=True)
    print("  Top 10 files by code lines:")
    print(f"  {'-' * 66}")
    for i, (code, total, path) in enumerate(file_ranks[:10], 1):
        rel = path.relative_to(root)
        print(f"  {i:>2}. {str(rel):<50} {code:>6} lines")
    print()


if __name__ == "__main__":
    main()
