#!/usr/bin/env python3
import os
import re
import sys
import argparse

# Comment detection regex patterns
DEFAULT_PATTERNS = [
    # Pattern 1: Excessive decorative lines / section separators
    r'^\s*(//|#)\s*[─━═\-_~*=\s🚀✨📝]{2,}.*[─━═\-_~*=\s]{2,}\s*$',
    r'^\s*(//|#)\s*[─━═\-_~*=\s🚀✨📝]{3,}.*$',
    
    # Pattern 2: Numbering and bullet lists
    r'^\s*(//|#)\s*\d+[\.\),]\s*.*$',
    
    # Pattern 3: AI-specific excessive defensive / boilerplate phrases
    r'^\s*(//|#)\s*.*(just do|tolerate|receive|guarantee|prevent|ensure|already completed|~type:|🚀|invisible dummy).*$',
    
    # Pattern 4: Play-by-play commentary of source code behavior
    r'^\s*(//|#)\s*.*(aim to|measures for|intent of|maintain consistency|responsible for|control|management|propagation|release|merge|clamp|apply|determination|migration|monospace font).*$',
    
    # Pattern 5: Hyphen separator lines
    r'^\s*(//|#)\s*---\s*.*[^-\s]+.*\s*---$',
    
    # Pattern 6: Explanatory long sentences (judging by ending)
    r'^\s*(//|#)\s*.*(therefore,|need to|must|must stay|originates from|is configured as|wait for\)|prevent\.\)).?$',
]

# Whitelist (false positive protection keywords)
PROTECTION_KEYWORDS = [
    "FIX-", "TODO", "FIXME", "BUG", "Issue", "O(1)", "av_frame_ref", 
    "SIGSEGV", "underflow", "NaN/Inf", "KDE", "null access prevention", 
    "prevent null access", "null pointer dereference"
]

def clean_file(file_path, patterns, dry_run=True, exclude_keywords=None):
    try:
        with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
            lines = f.readlines()
    except Exception as e:
        print(f"Skipped (Read Error) {file_path}: {e}")
        return 0

    new_lines = []
    removed_comments = []
    
    all_excludes = list(PROTECTION_KEYWORDS)
    if exclude_keywords:
        all_excludes.extend(exclude_keywords)

    for line_num, line in enumerate(lines, 1):
        is_contaminated = False
        
        if re.match(r'^\s*(//|#)', line):
            if any(kw in line for kw in all_excludes):
                new_lines.append(line)
                continue

            for pattern in patterns:
                if re.match(pattern, line):
                    is_contaminated = True
                    break
        
        if is_contaminated:
            removed_comments.append((line_num, line.strip()))
            continue
        else:
            new_lines.append(line)

    if removed_comments:
        print(f"[{'MATCH' if dry_run else 'REMOVE'}] {file_path} ({len(removed_comments)} items)")
        for num, text in removed_comments:
            print(f"  L{num}: {text}")
            
        if not dry_run:
            try:
                with open(file_path, 'w', encoding='utf-8') as f:
                    f.writelines(new_lines)
            except Exception as e:
                print(f"Error (Write Error) {file_path}: {e}")
                
    return len(removed_comments)

def main():
    parser = argparse.ArgumentParser(description="LLM-generated comment cleaner")
    parser.add_argument("dir", nargs="?", default="./", help="Target directory (default: current)")
    parser.add_argument("-x", "--execute", action="store_true", help="Execute deletion (default: dry-run)")
    parser.add_argument("-e", "--ext", nargs="+", default=[".cpp", ".hpp", ".qml", ".frag", ".vert", ".glsl", ".json", ".js"], help="Target extensions")
    parser.add_argument("--all-files", action="store_true", help="Scan all files regardless of extension")
    parser.add_argument("--add-pattern", nargs="+", default=[], help="Add custom regex patterns")
    parser.add_argument("--add-keyword", nargs="+", default=[], help="Add custom keywords")
    parser.add_argument("--exclude-keyword", nargs="+", default=[], help="Add protection keywords")

    args = parser.parse_args()

    if not os.path.exists(args.dir):
        print(f"Error: Path '{args.dir}' not found.", file=sys.stderr)
        sys.exit(1)

    active_patterns = list(DEFAULT_PATTERNS)
    for p in args.add_pattern:
        active_patterns.append(p)
    for kw in args.add_keyword:
        active_patterns.append(r'^\s*(//|#)\s*.*' + re.escape(kw) + r'.*$')

    dry_run = not args.execute
    
    print("--------------------------------------------------------------------------------")
    print(f"Starting scanner: {'DRY-RUN MODE' if dry_run else 'EXECUTE MODE'}")
    print(f"Target directory: {os.path.abspath(args.dir)}")
    print("--------------------------------------------------------------------------------")

    total_detected = 0
    file_count = 0
    target_extensions = tuple(args.ext)

    for root, dirs, files in os.walk(args.dir):
        if any(ignored in root for ignored in ['.git', 'build', '.build_tmp', 'vcpkg']):
            continue

        for file in files:
            file_path = os.path.join(root, file)
            should_scan = args.all_files or file.endswith(target_extensions)
            
            if should_scan:
                detected = clean_file(file_path, active_patterns, dry_run=dry_run, exclude_keywords=args.exclude_keyword)
                if detected > 0:
                    total_detected += detected
                    file_count += 1

    print("--------------------------------------------------------------------------------")
    if dry_run:
        print(f"Scan finished. Found {total_detected} comments in {file_count} files.")
        print("Run with '-x' or '--execute' to permanently delete these comments.")
    else:
        print(f"Cleanup finished. Removed {total_detected} comments across {file_count} files.")
    print("--------------------------------------------------------------------------------")

if __name__ == "__main__":
    main()