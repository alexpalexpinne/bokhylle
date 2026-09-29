#!/usr/bin/env python3
"""Generate and verify the HTTP API contract and frontend bindings."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROUTERS = ROOT / "crates/bokhylle-server/src/routes/registry"
SPEC = ROOT / "openapi.json"
TYPES = ROOT / "frontend/src/api/generated.ts"
METHODS = {"get", "post", "put", "patch", "delete"}


def registered_operations() -> set[tuple[str, str]]:
    found: set[tuple[str, str]] = set()
    for router in sorted(ROUTERS.glob("*.rs")):
        source = router.read_text()
        for match in re.finditer(r'\.(api_route|route)\(\s*"(/api/[^\"]+)"', source):
            kind, path = match.groups()
            if path == "/api/{*rest}":
                continue
            if kind != "api_route":
                raise ValueError(f"HTTP API route lacks OpenAPI registration: {path}")
            start = source.index("(", match.start())
            depth = 0
            end = None
            for index in range(start, len(source)):
                depth += (source[index] == "(") - (source[index] == ")")
                if depth == 0:
                    end = index + 1
                    break
            if end is None:
                raise ValueError(f"unclosed route registration: {path}")
            body = source[start:end]
            methods = re.findall(r"(?:routing::|\.)(get|post|put|patch|delete)\(", body)
            if not methods:
                raise ValueError(f"route has no recognized HTTP method: {path}")
            found.update((path, method) for method in methods)
    return found


def check_document(document: dict) -> None:
    if document.get("openapi") != "3.1.0":
        raise ValueError("expected OpenAPI 3.1")
    paths = document.get("paths", {})
    operations = {
        (path, method)
        for path, item in paths.items()
        for method in item
        if method in METHODS
    }
    registered = registered_operations()
    if operations != registered:
        raise ValueError(
            f"route/spec mismatch: missing={sorted(registered - operations)}, "
            f"extra={sorted(operations - registered)}"
        )
    identifiers = set()
    for path, method in sorted(operations):
        operation = paths[path][method]
        identifier = operation.get("operationId")
        if not identifier or identifier in identifiers:
            raise ValueError(f"missing or duplicate operationId: {method} {path}")
        identifiers.add(identifier)
        responses = operation.get("responses", {})
        if not any(code.startswith("2") for code in responses):
            raise ValueError(f"missing success response: {method} {path}")
        for code, response in responses.items():
            if not code.startswith("2"):
                continue
            media = response.get("content", {}).get("application/json")
            if media is not None and not isinstance(media.get("schema"), dict):
                raise ValueError(f"unconstrained JSON success response: {method} {path} {code}")
        if "default" not in responses:
            raise ValueError(f"missing error response: {method} {path}")
        if "security" not in operation:
            raise ValueError(f"missing auth declaration: {method} {path}")
        path_parameters = {
            parameter.get("name")
            for parameter in operation.get("parameters", [])
            if parameter.get("in") == "path" and parameter.get("required") is True
        }
        required_parameters = set(re.findall(r"\{([^}]+)\}", path))
        if path_parameters != required_parameters:
            raise ValueError(f"path parameter mismatch: {method} {path}")

    def check_refs(value: object) -> None:
        if isinstance(value, dict):
            reference = value.get("$ref")
            if isinstance(reference, str) and reference.startswith("#/"):
                target: object = document
                for part in reference[2:].split("/"):
                    part = part.replace("~1", "/").replace("~0", "~")
                    if not isinstance(target, dict) or part not in target:
                        raise ValueError(f"unresolved OpenAPI reference: {reference}")
                    target = target[part]
            for child in value.values():
                check_refs(child)
        elif isinstance(value, list):
            for child in value:
                check_refs(child)

    check_refs(document)

    schemas = document.get("components", {}).get("schemas", {})

    def component_refs(value: object) -> set[str]:
        if isinstance(value, dict):
            reference = value.get("$ref", "")
            found = (
                {reference.removeprefix("#/components/schemas/")}
                if isinstance(reference, str) and reference.startswith("#/components/schemas/")
                else set()
            )
            return found.union(*(component_refs(child) for child in value.values()))
        if isinstance(value, list):
            return set().union(*(component_refs(child) for child in value))
        return set()

    def reachable(roots: set[str]) -> set[str]:
        found = set(roots)
        while True:
            expanded = found | set().union(*(component_refs(schemas[name]) for name in found))
            if expanded == found:
                return found
            found = expanded

    request_roots = set().union(
        *(
            component_refs(paths[path][method].get("requestBody", {}))
            for path, method in operations
        )
    )
    response_roots = set().union(
        *(
            component_refs(response)
            for path, method in operations
            for code, response in paths[path][method]["responses"].items()
            if code.startswith("2")
        )
    )
    shared_objects = {
        name
        for name in reachable(request_roots) & reachable(response_roots)
        if schemas[name].get("properties")
    }
    if shared_objects:
        raise ValueError(
            f"request/response object schemas are shared: {sorted(shared_objects)}; "
            "split them before adjusting response field requiredness"
        )


def run() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="regenerate checked-in files")
    args = parser.parse_args()

    generated = subprocess.check_output(
        ["cargo", "run", "--quiet", "-p", "bokhylle-server", "--", "--openapi"],
        cwd=ROOT,
    )
    document = json.loads(generated)
    check_document(document)
    if args.write:
        SPEC.write_bytes(generated)
    elif not SPEC.exists() or SPEC.read_bytes() != generated:
        raise ValueError("openapi.json is stale; run `make openapi`")

    with tempfile.TemporaryDirectory() as directory:
        target = Path(directory) / "generated.ts"
        subprocess.run(
            ["pnpm", "-C", "frontend", "exec", "openapi-typescript", "../openapi.json", "-o", str(target)],
            cwd=ROOT,
            check=True,
            stdout=subprocess.DEVNULL,
        )
        if args.write:
            TYPES.write_bytes(target.read_bytes())
        elif not TYPES.exists() or TYPES.read_bytes() != target.read_bytes():
            raise ValueError("frontend/src/api/generated.ts is stale; run `make openapi`")
    print(f"OpenAPI contract current: {len(registered_operations())} operations")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(run())
    except (ValueError, subprocess.CalledProcessError) as error:
        print(f"OpenAPI check failed: {error}", file=sys.stderr)
        sys.exit(1)
