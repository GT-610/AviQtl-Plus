#!/usr/bin/env python3
"""
check.py - Script to run cppcheck across the entire project
Usage:
    python check.py [options]

Options:
    --jobs N        Number of parallel jobs (default: CPU cores)
    --level LEVEL   Check level: normal / all (default: normal)
    --xml           Output results in XML format (saved to check_result.xml)
    --fix           Auto-fix fixable warnings (experimental)
    --suppress FILE Specify suppress list file (default: .cppcheck_suppress)
    --build-dir DIR Build directory containing compile_commands.json
    --full          Ultimate analysis mode (max level for all tools + auto-fix)
    --clazy-level L Clazy check level (1-3, default: 1)
    --import-path P QML import path (can be specified multiple times)
    --skip-qml      Skip QML checks
    --skip-cppcheck Skip Cppcheck
    --skip-clazy    Skip Clazy
    --skip-tidy     Skip Clang-Tidy
    --skip-format   Skip code formatting
"""

import argparse
import concurrent.futures
import multiprocessing
import os
import shutil
import subprocess
import sys
from pathlib import Path

RESET  = "\033[0m"
RED    = "\033[31m"
YELLOW = "\033[33m"
GREEN  = "\033[32m"
CYAN   = "\033[36m"
BOLD   = "\033[1m"

def find_cpp_files(root: Path) -> list[Path]:
    exclude_dirs = {".git", ".build_tmp", "build", "dist", ".cache"}
    result = []
    for path in root.rglob("*.cpp"):
        if any(p in exclude_dirs for p in path.parts):
            continue
        # Exclude Qt auto-generated files
        if path.name.startswith("moc_") or path.name.startswith("qrc_"):
            continue
        result.append(path)
    return sorted(result)

def find_qml_files(root: Path) -> list[Path]:
    exclude_dirs = {".git", ".build_tmp", "build", "dist", ".cache"}
    result = []
    for path in root.rglob("*.qml"):
        if any(p in exclude_dirs for p in path.parts):
            continue
        result.append(path)
    return sorted(result)

def find_include_dirs(root: Path) -> list[Path]:
    dirs = set()
    for path in root.rglob("*.hpp"):
        if any(p in {".git", ".build_tmp", "build"} for p in path.parts):
            continue
        dirs.add(path.parent)
    return sorted(dirs)

def load_suppress_file(path: Path) -> list[str]:
    if not path.exists():
        return []
    lines = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            lines.append(line)
    return lines

def run_formatting(root: Path) -> int:
    print(f"{BOLD}{YELLOW}--- Code Formatting ---{RESET}")
    
    # C++ / HPP files
    cpp_hpp_patterns = ["*.cpp", "*.hpp"]
    exclude_dirs = {".git", ".build_tmp", "build", "dist", ".cache", "clap", "vst3sdk"}
    cpp_hpp_files = []
    for pattern in cpp_hpp_patterns:
        for path in root.rglob(pattern):
            if any(p in exclude_dirs for p in path.parts):
                continue
            if path.name.startswith(("moc_", "qrc_")):
                continue
            cpp_hpp_files.append(path)
    
    errors = 0
    if cpp_hpp_files and shutil.which("clang-format"):
        print(f"  Formatting C++/HPP ({len(cpp_hpp_files)} files)...")
        result = subprocess.run(["clang-format", "-i", "--"] + [str(f) for f in cpp_hpp_files], cwd=root, stderr=subprocess.STDOUT)
        if result.returncode != 0:
            errors += 1
    elif not shutil.which("clang-format"):
        print(f"{RED}Warning: clang-format not found.{RESET}")

    # QML files
    qml_files = find_qml_files(root)
    if qml_files and shutil.which("qmlformat"):
        print(f"  Formatting QML ({len(qml_files)} files)...")
        result = subprocess.run(["qmlformat", "-i", "--"] + [str(f) for f in qml_files], cwd=root, stderr=subprocess.STDOUT)
        if result.returncode != 0:
            errors += 1
    elif not shutil.which("qmlformat"):
        print(f"{RED}Warning: qmlformat not found.{RESET}")
    
    if errors == 0:
        print(f"{GREEN}  Formatting complete{RESET}\n")
    else:
        print(f"{YELLOW}  Formatting completed with {errors} error(s){RESET}\n")
    return errors

def run_cppcheck(
    files: list[Path],
    include_dirs: list[Path],
    suppress_list: list[str],
    args: argparse.Namespace,
    root: Path
) -> int:
    cmd = ["cppcheck"]

    # Number of parallel jobs
    cmd += [f"-j{args.jobs}"]

    # C++ standard
    cmd += ["--std=c++23"]

    if args.level == "all":
        cmd += ["--enable=all"]
    else:
        cmd += ["--enable=warning,performance,portability,information,style"]

    # Include directories
    for d in include_dirs:
        cmd += [f"-I{d}"]

    # Minimal defines for Qt / FFmpeg / LuaJIT (suppress false positives)
    defines = [
        "Q_OBJECT=", "Q_INVOKABLE=", "Q_PROPERTY(...)=", "Q_SIGNALS=protected",
        "Q_SLOTS=", "Q_EMIT=", "slots=", "signals=protected:",
    ]
    for d in defines:
        cmd += [f"-D{d}"]

    # Suppressions
    for s in suppress_list:
        cmd += [f"--suppress={s}"]
    # Common suppression for false positives from Qt generated code
    cmd += [
        "--suppress=missingIncludeSystem",
        "--suppress=unmatchedSuppression",
        "--suppress=unknownMacro",
    ]

    # Use compile_commands.json if available
    if args.build_dir:
        bd = Path(args.build_dir)
        cc = bd / "compile_commands.json"
        if cc.exists():
            cmd += [f"--project={cc}"]
        else:
            print(f"{YELLOW}Warning: {cc} not found. Running with file list.{RESET}")
            cmd += [str(f) for f in files]
    else:
        cmd += [str(f) for f in files]

    # XML output
    if args.xml:
        cmd += ["--xml", "--xml-version=2"]

    # Template (for normal text output)
    if not args.xml:
        cmd += ["--template={file}:{line}: [{severity}] {id}: {message}"]

    print(f"{BOLD}{YELLOW}--- Cppcheck ---{RESET}")
    result = subprocess.run(cmd, cwd=root)
    return result.returncode

def run_qmllint(files: list[Path], args: argparse.Namespace, root: Path) -> int:
    if not files:
        return 0
    print(f"{BOLD}{YELLOW}--- qmllint (including QQmlSA) ---{RESET}")
    if not shutil.which("qmllint"):
        print(f"{RED}Warning: qmllint not found. Skipping.{RESET}")
        return 0
    
    cmd = ["qmllint"]
    
    # Set import paths
    if args.import_path:
        for p in args.import_path:
            cmd += ["-I", p]
    
    if args.build_dir:
        for sub in ["qml", "AviQtl"]:
            qml_path = Path(args.build_dir) / sub
            if qml_path.exists():
                cmd += ["-I", str(qml_path)]

    cmd += [str(f) for f in files]
    result = subprocess.run(cmd, cwd=root, stderr=subprocess.STDOUT)
    return result.returncode

def run_clazy(files: list[Path], args: argparse.Namespace, root: Path) -> int:
    print(f"{BOLD}{YELLOW}--- Clazy (Qt Anti-Patterns) ---{RESET}")
    executable = shutil.which("clazy-standalone") or shutil.which("clazy")
    if not executable:
        print(f"{RED}Warning: clazy not found. Skipping.{RESET}")
        return 0

    if not args.build_dir:
        print(f"{RED}Error: --build-dir is required to run Clazy.{RESET}")
        return 1

    cc_json = Path(args.build_dir) / "compile_commands.json"
    if not cc_json.exists():
        print(f"{RED}Error: {cc_json} not found.{RESET}")
        return 1

    if args.full:
        clazy_checks = ["level1", "level2"]
    else:
        clazy_checks = [f"level{i}" for i in range(1, min(args.clazy_level, 2) + 1)]

    # Remove unsupported check names
    checks_str = ",".join(clazy_checks + ["qstring-allocations"])
    cmd = [executable, f"-p={args.build_dir}", f"-checks={checks_str}"] + [str(f) for f in files]
    
    # Very heavy; let OS scheduler handle parallel execution or split as needed
    result = subprocess.run(cmd, cwd=root, stderr=subprocess.STDOUT)
    return result.returncode

def _invoke_tidy(file_path: str, build_dir: str, checks: str, fix: bool) -> int:
    """Run clang-tidy on an individual file (for parallel processing)"""
    cmd = ["clang-tidy", f"-p={build_dir}", f"--checks={checks}", "--quiet"]
    if fix:
        cmd.append("--fix-errors") # Attempt more aggressive fixes during --full
    cmd.append(file_path)
    
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.stdout:
        sys.stdout.write(res.stdout)
    if res.stderr:
        sys.stdout.write(res.stderr)
    return res.returncode

def run_clang_tidy(files: list[Path], args: argparse.Namespace, root: Path) -> int:
    print(f"{BOLD}{YELLOW}--- Clang-Tidy (General & Static Analyzer) ---{RESET}")
    if not shutil.which("clang-tidy"):
        print(f"{RED}Warning: clang-tidy not found. Skipping.{RESET}")
        return 0

    if not args.build_dir:
        print(f"{RED}Error: --build-dir is required to run Clang-Tidy.{RESET}")
        return 1

    # Include Qt-specific checks
    if args.full:
        checks = "bugprone-*,performance-*,portability-*,modernize-*,clang-analyzer-*,qt-*,readability-*,cppcoreguidelines-*,cert-*"
    else:
        checks = "bugprone-*,performance-*,portability-*,modernize-*,clang-analyzer-*,qt-*"
    
    print(f"  Running in parallel with {args.jobs} threads...")
    errors = 0
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as executor:
            futures = [
                executor.submit(_invoke_tidy, str(f), args.build_dir, checks, args.fix)
                for f in files
            ]
            for future in concurrent.futures.as_completed(futures):
                if future.result() != 0:
                    errors += 1
        return 0 if errors == 0 else 1
    except Exception as e:
        print(f"{RED}Error occurred during Clang-Tidy execution: {e}{RESET}")
        return 1



def print_summary(returncode: int, xml_path: Path | None) -> None:
    print()
    if returncode == 0:
        print(f"{BOLD}{GREEN}✔  cppcheck complete: No issues{RESET}")
    else:
        print(f"{BOLD}{RED}✘  cppcheck complete: Warnings/errors found (exit code {returncode}){RESET}")
    if xml_path and xml_path.exists():
        print(f"{CYAN}   XML Report: {xml_path}{RESET}")

def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run cppcheck across AviQtl project"
    )
    parser.add_argument(
        "--jobs", type=int,
        default=multiprocessing.cpu_count(),
        help="Number of parallel jobs (default: CPU cores)"
    )
    parser.add_argument(
        "--level", choices=["normal", "all"], default="normal",
        help="Check level"
    )
    parser.add_argument(
        "--xml", action="store_true",
        help="Output in XML format"
    )
    parser.add_argument(
        "--suppress", default=".cppcheck_suppress",
        metavar="FILE",
        help="Suppress list file (default: .cppcheck_suppress)"
    )
    parser.add_argument(
        "--build-dir", default=None,
        metavar="DIR",
        help="Build directory containing compile_commands.json"
    )
    parser.add_argument(
        "--full", action="store_true", help="Ultimate analysis mode (max level for all tools + auto-fix)"
    )
    parser.add_argument(
        "--clazy-level", type=int, choices=[1, 2], default=1,
        help="Clazy check level (1-2)"
    )
    parser.add_argument(
        "--import-path", action="append",
        metavar="PATH",
        help="QML import path (can be specified multiple times)"
    )
    parser.add_argument(
        "--skip-qml", action="store_true", help="Skip QML checks"
    )
    parser.add_argument(
        "--skip-cppcheck", action="store_true", help="Skip Cppcheck"
    )
    parser.add_argument(
        "--skip-clazy", action="store_true", help="Skip Clazy"
    )
    parser.add_argument(
        "--skip-tidy", action="store_true", help="Skip Clang-Tidy"
    )
    parser.add_argument(
        "--skip-format", action="store_true", help="Skip code formatting"
    )
    parser.add_argument(
        "--fix", action="store_true", help="Auto-fix fixable warnings"
    )

    args = parser.parse_args()

    # Overrides when --full mode is active
    if args.full:
        args.level = "all"
        args.clazy_level = 2
        args.fix = True

    # Check if cppcheck is available
    if not shutil.which("cppcheck"):
        print(f"{RED}Error: cppcheck not found.{RESET}")
        print("  CachyOS: sudo pacman -S cppcheck")
        return 1

    root = Path(__file__).parent.resolve()

    print(f"{BOLD}{CYAN}=== AviQtl cppcheck ==={RESET}")
    print(f"  Root        : {root}")
    print(f"  Jobs        : {args.jobs}")
    print(f"  Level       : {args.level}")

    # 0. Formatting (unified process)
    total_errors = 0
    if not args.skip_format:
        total_errors += run_formatting(root)

    # Collect files
    cpp_files    = find_cpp_files(root)
    qml_files    = find_qml_files(root)
    include_dirs = find_include_dirs(root)
    suppress     = load_suppress_file(root / args.suppress)

    print(f"  .cpp files   : {len(cpp_files)}")
    print(f"  .qml files   : {len(qml_files)}")
    print(f"  Includes     : {len(include_dirs)} dirs")
    if suppress:
        print(f"  Suppressions : {len(suppress)}")
    print()

    # 1 & 2. QML Lint (QQmlSA)
    if not args.skip_qml:
        total_errors += run_qmllint(qml_files, args, root)

    # 3. Clazy
    if not args.skip_clazy:
        total_errors += run_clazy(cpp_files, args, root)

    # 4. Clang-Tidy (Includes Static Analyzer)
    if not args.skip_tidy:
        total_errors += run_clang_tidy(cpp_files, args, root)

    # Cppcheck
    if not args.skip_cppcheck:
        # Backward compatibility: XML output is Cppcheck-only
        if args.xml:
            print(f"{CYAN}Running Cppcheck in XML mode...{RESET}")
        total_errors += run_cppcheck(cpp_files, include_dirs, suppress, args, root)

    print_summary(total_errors, root / "check_result.xml" if args.xml else None)
    
    return 1 if total_errors > 0 else 0

if __name__ == "__main__":
    sys.exit(main())
