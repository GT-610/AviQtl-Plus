#!/usr/bin/env python3
"""Verify that the hand-written C++ declarations match the Rust C ABI."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


RUST_PRIMITIVES = {
    "u8": "u8",
    "u16": "u16",
    "u32": "u32",
    "u64": "u64",
    "i32": "i32",
    "usize": "usize",
    "f32": "f32",
    "f64": "f64",
    "()": "void",
}

C_PRIMITIVES = {
    "std::uint8_t": "u8",
    "std::uint16_t": "u16",
    "std::uint32_t": "u32",
    "std::uint64_t": "u64",
    "std::int32_t": "i32",
    "std::size_t": "usize",
    "float": "f32",
    "double": "f64",
    "void": "void",
}


def _matching_delimiter(text: str, start: int, opening: str, closing: str) -> int:
    depth = 0
    for index in range(start, len(text)):
        if text[index] == opening:
            depth += 1
        elif text[index] == closing:
            depth -= 1
            if depth == 0:
                return index
    raise ValueError(f"unclosed {opening!r} at byte {start}")


def _split_top_level(text: str) -> list[str]:
    parts: list[str] = []
    start = 0
    depths = {"(": 0, "[": 0, "<": 0}
    closings = {")": "(", "]": "[", ">": "<"}
    for index, character in enumerate(text):
        if character in depths:
            depths[character] += 1
        elif character in closings:
            depths[closings[character]] -= 1
        elif character == "," and all(depth == 0 for depth in depths.values()):
            parts.append(text[start:index].strip())
            start = index + 1
    final = text[start:].strip()
    if final:
        parts.append(final)
    return parts


def _rust_type(type_name: str) -> str:
    type_name = " ".join(type_name.strip().split())
    array = re.fullmatch(r"\[(.+);\s*(\d+)\]", type_name)
    if array is not None:
        return f"[{_rust_type(array.group(1))};{array.group(2)}]"
    if type_name.startswith("*const "):
        return f"*const<{_rust_type(type_name[7:])}>"
    if type_name.startswith("*mut "):
        return f"*mut<{_rust_type(type_name[5:])}>"
    return RUST_PRIMITIVES.get(type_name, type_name)


def _c_type(type_name: str) -> str:
    compact = " ".join(type_name.strip().split())
    pointer_count = compact.count("*")
    is_const = compact.startswith("const ")
    base = compact.replace("*", " ")
    if is_const:
        base = base[6:]
    base = " ".join(base.split())
    result = C_PRIMITIVES.get(base, base)
    for pointer_index in range(pointer_count):
        qualifier = "const" if pointer_index == 0 and is_const else "mut"
        result = f"*{qualifier}<{result}>"
    return result


def _c_declaration_type(declaration: str) -> tuple[str, str]:
    match = re.search(r"([A-Za-z_]\w*)\s*(?:\[\s*(\d+)\s*\])?\s*$", declaration.strip())
    if match is None:
        raise ValueError(f"cannot parse C++ declaration: {declaration}")
    field_type = _c_type(declaration[: match.start()])
    if match.group(2) is not None:
        field_type = f"[{field_type};{match.group(2)}]"
    return match.group(1), field_type


def parse_rust_functions(sources: dict[str, str]) -> dict[str, tuple[str, tuple[str, ...]]]:
    functions: dict[str, tuple[str, tuple[str, ...]]] = {}
    pattern = re.compile(r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(aviqtl_\w+)\s*\(')
    for path, source in sources.items():
        for match in pattern.finditer(source):
            name = match.group(1)
            attribute_prefix = source[: match.start()]
            marker = max(
                attribute_prefix.rfind("#[unsafe(no_mangle)]"),
                attribute_prefix.rfind("#[no_mangle]"),
            )
            if marker < 0 or marker < attribute_prefix.rfind('extern "C" fn'):
                raise ValueError(f"Rust ABI function {name} in {path} is missing #[unsafe(no_mangle)]")
            close = _matching_delimiter(source, match.end() - 1, "(", ")")
            parameters = []
            for parameter in _split_top_level(source[match.end() : close]):
                if ":" not in parameter:
                    raise ValueError(f"cannot parse Rust parameter for {name} in {path}: {parameter}")
                parameters.append(_rust_type(parameter.split(":", 1)[1]))
            suffix = source[close + 1 : source.find("{", close)]
            return_match = re.search(r"->\s*(.+?)\s*$", suffix, re.DOTALL)
            return_type = _rust_type(return_match.group(1)) if return_match else "void"
            if name in functions:
                raise ValueError(f"duplicate Rust ABI export {name}")
            functions[name] = (return_type, tuple(parameters))
    return functions


def parse_c_functions(header: str) -> dict[str, tuple[str, tuple[str, ...]]]:
    functions: dict[str, tuple[str, tuple[str, ...]]] = {}
    pattern = re.compile(
        r"^[ \t]*((?:const\s+)?(?:std::(?:u?int(?:8|16|32|64)_t|size_t)|"
        r"double|float|void|AviQtl\w+)(?:\s*\*)*)\s*(aviqtl_\w+)\s*\(",
        re.MULTILINE,
    )
    for match in pattern.finditer(header):
        return_type = _c_type(match.group(1))
        name = match.group(2)
        close = _matching_delimiter(header, match.end() - 1, "(", ")")
        parameters = []
        for parameter in _split_top_level(header[match.end() : close]):
            if parameter == "void":
                continue
            _, parameter_type = _c_declaration_type(parameter)
            parameters.append(parameter_type)
        functions[name] = (return_type, tuple(parameters))
    return functions


def parse_rust_structs(sources: dict[str, str]) -> dict[str, tuple[tuple[str, str], ...]]:
    structs: dict[str, tuple[tuple[str, str], ...]] = {}
    pattern = re.compile(r"#\[repr\(C\)\](?:(?!#\[repr).)*?pub\s+struct\s+(AviQtl\w+)\s*\{", re.DOTALL)
    for source in sources.values():
        for match in pattern.finditer(source):
            name = match.group(1)
            close = _matching_delimiter(source, match.end() - 1, "{", "}")
            fields = []
            for field in _split_top_level(source[match.end() : close]):
                field_match = re.fullmatch(r"pub\s+([A-Za-z_]\w*)\s*:\s*(.+)", field, re.DOTALL)
                if field_match is None:
                    raise ValueError(f"cannot parse Rust field in {name}: {field}")
                fields.append((field_match.group(1), _rust_type(field_match.group(2))))
            structs[name] = tuple(fields)
    return structs


def parse_c_structs(header: str) -> dict[str, tuple[tuple[str, str], ...]]:
    structs: dict[str, tuple[tuple[str, str], ...]] = {}
    pattern = re.compile(r"struct\s+(AviQtl\w+)\s*\{")
    for match in pattern.finditer(header):
        name = match.group(1)
        close = _matching_delimiter(header, match.end() - 1, "{", "}")
        fields = []
        for declaration in header[match.end() : close].split(";"):
            declaration = declaration.strip()
            if declaration:
                field_name, field_type = _c_declaration_type(declaration)
                fields.append((field_name, field_type))
        structs[name] = tuple(fields)
    return structs


def _constant_table(text: str, pattern: str) -> dict[str, int]:
    return {match.group(1): int(match.group(2)) for match in re.finditer(pattern, text)}


def compare_contract(sources: dict[str, str], header: str) -> list[str]:
    errors: list[str] = []
    rust_functions = parse_rust_functions(sources)
    c_functions = parse_c_functions(header)
    for name in sorted(rust_functions.keys() - c_functions.keys()):
        errors.append(f"Rust export missing from C++ header: {name}")
    for name in sorted(c_functions.keys() - rust_functions.keys()):
        errors.append(f"C++ declaration missing from Rust exports: {name}")
    for name in sorted(rust_functions.keys() & c_functions.keys()):
        if rust_functions[name] != c_functions[name]:
            errors.append(
                f"signature mismatch for {name}: Rust {rust_functions[name]}, C++ {c_functions[name]}"
            )

    rust_structs = parse_rust_structs(sources)
    c_structs = parse_c_structs(header)
    for name in sorted(rust_structs.keys() - c_structs.keys()):
        errors.append(f"Rust #[repr(C)] struct missing from C++ header: {name}")
    for name in sorted(c_structs.keys() - rust_structs.keys()):
        errors.append(f"C++ ABI struct missing from Rust #[repr(C)] definitions: {name}")
    for name in sorted(rust_structs.keys() & c_structs.keys()):
        if rust_structs[name] != c_structs[name]:
            errors.append(
                f"field mismatch for {name}: Rust {rust_structs[name]}, C++ {c_structs[name]}"
            )

    rust = "\n".join(sources.values())
    rust_version = re.search(r"pub const ABI_VERSION:\s*u32\s*=\s*(\d+)\s*;", rust)
    c_version = re.search(r"AVIQTL_RUST_CORE_ABI_VERSION\s*=\s*(\d+)", header)
    if rust_version is None or c_version is None or rust_version.group(1) != c_version.group(1):
        errors.append("ABI version constant mismatch")

    rust_capabilities = _constant_table(rust, r"pub const CAPABILITY_(\w+):\s*u64\s*=\s*1\s*<<\s*(\d+)\s*;")
    c_capabilities = _constant_table(header, r"AVIQTL_RUST_CORE_CAPABILITY_(\w+)\s*=\s*1ULL\s*<<\s*(\d+)")
    if rust_capabilities != c_capabilities:
        errors.append(f"capability table mismatch: Rust {rust_capabilities}, C++ {c_capabilities}")
    aggregate = re.search(r"pub const CAPABILITIES:\s*u64\s*=\s*(.+?);", rust, re.DOTALL)
    aggregate_names = set(re.findall(r"CAPABILITY_(\w+)", aggregate.group(1))) if aggregate else set()
    if aggregate_names != rust_capabilities.keys():
        errors.append("Rust CAPABILITIES aggregate does not include every declared capability")

    rust_statuses = _constant_table(rust, r"pub const STATUS_(\w+):\s*u32\s*=\s*(\d+)\s*;")
    c_statuses = _constant_table(header, r"AVIQTL_RUST_CORE_STATUS_(\w+)\s*=\s*(\d+)")
    if rust_statuses != c_statuses:
        errors.append(f"status table mismatch: Rust {rust_statuses}, C++ {c_statuses}")
    return errors


def load_rust_sources(source_root: Path) -> dict[str, str]:
    return {
        str(path.relative_to(source_root)): path.read_text(encoding="utf-8")
        for path in sorted(source_root.rglob("*.rs"))
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust-src", type=Path, required=True)
    parser.add_argument("--header", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        errors = compare_contract(
            load_rust_sources(arguments.rust_src),
            arguments.header.read_text(encoding="utf-8"),
        )
    except (OSError, ValueError) as error:
        print(f"Rust C ABI check failed: {error}", file=sys.stderr)
        return 1
    if errors:
        print("Rust C ABI contract mismatch:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print("Rust C ABI contract is consistent.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
