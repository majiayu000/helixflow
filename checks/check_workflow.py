#!/usr/bin/env python3
"""Validate a SpecRail workflow pack without network or GitHub writes."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

from specrail_lib import (
    SpecRailError,
    load_pack,
    read_text,
    validate_action_policy,
    validate_json_schemas,
    validate_labels,
    validate_state_graph,
    validate_skills_lock,
    validate_template_parity,
)


REQUIRED_FILES = [
    "README.md",
    "LICENSE",
    "CHANGELOG.md",
    "SPEC.md",
    "docs/ADOPTION_MATRIX.md",
    "workflow.yaml",
    "states.yaml",
    "labels.yaml",
    "examples/adoptions/matrix.json",
    "examples/fixtures/issue-ready-to-implement.json",
    "examples/fixtures/issue-ready-to-spec.json",
    "examples/fixtures/issue-reserved-internal.json",
    "examples/fixtures/issue-body-hint-ready-to-implement.json",
    "examples/fixtures/pr-clean-authorized.json",
    "examples/fixtures/pr-diff.patch",
    "examples/fixtures/pr-missing-human-auth.json",
    "examples/fixtures/pr-pending-ci.json",
    "examples/fixtures/pr-unresolved-thread.json",
    "examples/fixtures/review-invalid-body.json",
    "examples/fixtures/review-invalid-empty-suggestion.json",
    "examples/fixtures/review-invalid-line.json",
    "examples/fixtures/review-invalid-range.json",
    "examples/fixtures/review-invalid-severity.json",
    "examples/fixtures/review-invalid-suggestion-side.json",
    "examples/fixtures/review-spec-drift.json",
    "examples/fixtures/review-valid.json",
    "checks/github_issue_evidence.py",
    "checks/github_pr_evidence.py",
    "checks/pr_gate.py",
    "checks/review_json_gate.py",
    "tools/install_codex_skills.py",
    "skills-lock.json",
    "templates/issue_bug.md",
    "templates/issue_feature.md",
    "templates/product_spec.md",
    "templates/tech_spec.md",
    "templates/tasks.md",
    "templates/pull_request.md",
    "templates/zh-CN/issue_bug.md",
    "templates/zh-CN/issue_feature.md",
    "templates/zh-CN/product_spec.md",
    "templates/zh-CN/tech_spec.md",
    "templates/zh-CN/tasks.md",
    "templates/zh-CN/pull_request.md",
    "templates/zh-CN/tranche_checkpoint.md",
    "review/agent_first_review.md",
    "review/human_final_review.md",
    "policies/security_disclosure.md",
    "policies/maintainer_escalation.md",
    "schemas/flow_manifest.schema.json",
    "schemas/issue_triage.schema.json",
    "schemas/issue_evidence.schema.json",
    "schemas/evaluation_result.schema.json",
    "schemas/adoption_matrix.schema.json",
    "schemas/spec_packet.schema.json",
    "schemas/task_plan.schema.json",
    "schemas/pr_review_gate.schema.json",
    "schemas/review_result.schema.json",
    "schemas/runtime_checkpoint.schema.json",
    "schemas/workflow_run.schema.json",
    "checks/runtime_ledger_gate.py",
    "templates/tranche_checkpoint.md",
]

REQUIRED_TOKENS = {
    "workflow.yaml": [
        "default_mode: dry_run",
        "forbidden_agent_actions:",
        "required_human_gates:",
        "action_policy:",
    ],
    "states.yaml": [
        "ready_to_spec",
        "ready_to_implement",
        "agent_review",
        "human_review",
        "merge_ready",
    ],
    "labels.yaml": [
        "readiness:",
        "ready_to_spec",
        "ready_to_implement",
        "security_private",
    ],
    "templates/product_spec.md": [
        "## Goals",
        "## Non-Goals",
        "## Acceptance Criteria",
    ],
    "templates/tech_spec.md": [
        "## Proposed Design",
        "## Test Plan",
        "## Rollback Plan",
    ],
    "templates/tasks.md": [
        "## Implementation Tasks",
        "## Verification",
        "## Handoff Notes",
    ],
    "templates/pull_request.md": [
        "## Linked Work",
        "## Readiness Gate",
        "## Review Gate",
        "## Merge Gate",
        "## Verification",
    ],
}


def issue_token_display(issue_number: str) -> str:
    return f"GH-{issue_number} or GH{issue_number} or #{issue_number} or issues/{issue_number}"


def has_linked_issue_token(text: str, issue_number: str) -> bool:
    patterns = [
        rf"(?<![A-Za-z0-9_-])GH-{re.escape(issue_number)}(?![A-Za-z0-9_-])",
        rf"(?<![A-Za-z0-9_-])GH{re.escape(issue_number)}(?![A-Za-z0-9_-])",
        rf"(?<![A-Za-z0-9_-])#{re.escape(issue_number)}(?![0-9])",
        rf"(?<![A-Za-z0-9_-])issues/{re.escape(issue_number)}(?![A-Za-z0-9_-])",
    ]
    return any(re.search(pattern, text) for pattern in patterns)


def validate_required_files(repo: Path) -> list[str]:
    errors: list[str] = []
    for rel in REQUIRED_FILES:
        path = repo / rel
        if not path.is_file():
            errors.append(f"missing required file: {rel}")
    return errors


def validate_tokens(repo: Path) -> list[str]:
    errors: list[str] = []
    for rel, tokens in REQUIRED_TOKENS.items():
        path = repo / rel
        if not path.is_file():
            continue
        text = read_text(path)
        for token in tokens:
            if token not in text:
                errors.append(f"{rel}: missing token {token!r}")
    return errors


def validate_spec_packet(spec_dir: Path) -> list[str]:
    errors: list[str] = []
    if not spec_dir.exists():
        return [f"spec packet does not exist: {spec_dir}"]
    if not spec_dir.is_dir():
        return [f"spec packet is not a directory: {spec_dir}"]

    match = re.fullmatch(r"GH([0-9]+)", spec_dir.name)
    if not match:
        errors.append(f"{spec_dir}: spec packet directory must be named GH<number>")
        issue_number = None
    else:
        issue_number = match.group(1)

    for name in ["product.md", "tech.md"]:
        path = spec_dir / name
        if not path.is_file():
            errors.append(f"{spec_dir}: missing {name}")
            continue
        text = read_text(path)
        if not text.strip():
            errors.append(f"{path}: must not be empty")
        if issue_number and not has_linked_issue_token(text, issue_number):
            errors.append(f"{path}: missing linked issue token {issue_token_display(issue_number)}")

    task_path = spec_dir / "tasks.md"
    if not task_path.is_file():
        errors.append(f"{spec_dir}: missing tasks.md")
    else:
        errors.extend(validate_task_plan(task_path, issue_number))
    return errors


def spec_packet_sort_key(spec_dir: Path) -> tuple[int, int, str]:
    match = re.fullmatch(r"GH([0-9]+)", spec_dir.name)
    if match:
        return (0, int(match.group(1)), spec_dir.name)
    return (1, 0, str(spec_dir))


def discover_spec_packet_dirs(repo: Path) -> list[Path]:
    specs_dir = repo / "specs"
    if not specs_dir.is_dir():
        return []
    return sorted(
        [
            path.resolve()
            for path in specs_dir.iterdir()
            if path.is_dir() and re.fullmatch(r"GH([0-9]+)", path.name)
        ],
        key=spec_packet_sort_key,
    )


def discover_changed_spec_packet_dirs(repo: Path, base_ref: str | None) -> list[Path]:
    if not base_ref:
        return []
    base_ref = base_ref.strip()
    if not base_ref or re.fullmatch(r"0+", base_ref):
        return []
    if base_ref.startswith("-") or not re.fullmatch(r"[A-Za-z0-9_./-]+", base_ref):
        raise SpecRailError(f"invalid changed spec base ref: {base_ref}")

    result = subprocess.run(
        ["git", "-C", str(repo), "diff", "--name-only", base_ref, "HEAD", "--", "specs/"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip() or result.stdout.strip()
        raise SpecRailError(f"failed to discover changed spec packets from {base_ref}: {stderr}")

    spec_dirs: set[Path] = set()
    for rel_path in result.stdout.splitlines():
        parts = Path(rel_path).parts
        if len(parts) >= 2 and parts[0] == "specs" and re.fullmatch(r"GH[0-9]+", parts[1]):
            spec_dirs.add((repo / parts[0] / parts[1]).resolve())
    return sorted(spec_dirs, key=spec_packet_sort_key)


def is_new_format_spec_packet(spec_dir: Path) -> bool:
    marker_files = ["product.md", "tech.md", "tasks.md"]
    markers = [
        "## Linked Issue",
        "## Implementation Tasks",
        "## 实现任务",
    ]
    for name in marker_files:
        path = spec_dir / name
        if not path.is_file():
            continue
        text = read_text(path)
        if any(marker in text for marker in markers):
            return True
    return False


def select_spec_packet_dirs_for_validation(
    repo: Path,
    raw_spec_dirs: list[str],
    *,
    all_specs: bool,
) -> list[tuple[Path, bool]]:
    spec_dirs: list[Path] = []
    if all_specs:
        spec_dirs.extend(discover_spec_packet_dirs(repo))
    explicit_spec_dirs = [(repo / raw_spec_dir).resolve() for raw_spec_dir in raw_spec_dirs]
    spec_dirs.extend(explicit_spec_dirs)

    unique_spec_dirs: list[tuple[Path, bool]] = []
    explicit_set = set(explicit_spec_dirs)
    seen: set[Path] = set()
    for spec_dir in spec_dirs:
        if spec_dir in seen:
            continue
        seen.add(spec_dir)
        strict = spec_dir in explicit_set or (all_specs and is_new_format_spec_packet(spec_dir))
        unique_spec_dirs.append((spec_dir, strict))

    if all_specs:
        return sorted(unique_spec_dirs, key=lambda item: spec_packet_sort_key(item[0]))
    return unique_spec_dirs


def merge_selected_spec_dirs(
    selected: list[tuple[Path, bool]],
    changed_spec_dirs: list[Path],
) -> list[tuple[Path, bool]]:
    merged: dict[Path, bool] = {spec_dir: strict for spec_dir, strict in selected}
    for spec_dir in changed_spec_dirs:
        merged[spec_dir] = True
    return sorted(merged.items(), key=lambda item: spec_packet_sort_key(item[0]))


def validate_task_id(path: Path, line_number: int, task_id: str, issue_number: str | None) -> list[str]:
    errors: list[str] = []
    prefix = f"SP{issue_number}-T" if issue_number else "SP"
    if issue_number and not task_id.startswith(prefix):
        errors.append(f"{path}:{line_number}: task ID {task_id} must start with {prefix}")
    return errors


def validate_checklist_tasks(path: Path, text: str, issue_number: str | None) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    ids: list[str] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        match = re.match(r"\s*-\s*\[[ xX]\]\s*`([^`]+)`", line)
        if not match:
            continue
        task_id = match.group(1)
        ids.append(task_id)
        errors.extend(validate_task_id(path, line_number, task_id, issue_number))
        for token in ["Owner:", "Done when:", "Verify:"]:
            if token not in line:
                errors.append(f"{path}:{line_number}: task {task_id} missing {token}")
    return ids, errors


def parse_markdown_table_row(line: str) -> list[str] | None:
    stripped = line.strip()
    if not stripped.startswith("|") or not stripped.endswith("|"):
        return None
    cells = [cell.strip() for cell in stripped.strip("|").split("|")]
    if not cells:
        return None
    if cells[0].lower() == "id":
        return None
    if all(re.fullmatch(r":?-{3,}:?", cell.replace(" ", "")) for cell in cells):
        return None
    return cells


def validate_table_tasks(path: Path, text: str, issue_number: str | None) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    ids: list[str] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        cells = parse_markdown_table_row(line)
        if not cells:
            continue
        task_id = cells[0].strip("`")
        if not re.fullmatch(r"SP[0-9]+-T[0-9A-Za-z_-]+", task_id):
            continue
        ids.append(task_id)
        errors.extend(validate_task_id(path, line_number, task_id, issue_number))
        if len(cells) < 6:
            errors.append(
                f"{path}:{line_number}: task {task_id} table row must have ID, Owner, "
                "Dependencies, Task, Done When, and Verify columns"
            )
            continue
        for index, name in [(1, "Owner"), (4, "Done when"), (5, "Verify")]:
            if not cells[index].strip():
                errors.append(f"{path}:{line_number}: task {task_id} missing {name}")
    return ids, errors


def validate_task_plan(path: Path, issue_number: str | None) -> list[str]:
    errors: list[str] = []
    text = read_text(path)
    if not text.strip():
        return [f"{path}: must not be empty"]

    checklist_ids, checklist_errors = validate_checklist_tasks(path, text, issue_number)
    table_ids, table_errors = validate_table_tasks(path, text, issue_number)
    ids = checklist_ids + table_ids
    errors.extend(checklist_errors)
    errors.extend(table_errors)

    if not ids:
        errors.append(f"{path}: no task checklist or table items found")
    duplicates = sorted({task_id for task_id in ids if ids.count(task_id) > 1})
    for duplicate in duplicates:
        errors.append(f"{path}: duplicate task ID {duplicate}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate a SpecRail workflow pack."
    )
    parser.add_argument("--repo", default=".", help="Workflow pack root")
    parser.add_argument(
        "--spec-dir",
        action="append",
        default=[],
        help="Optional specs/GH<number> directory to validate",
    )
    parser.add_argument(
        "--all-specs",
        action="store_true",
        help="Validate every specs/GH<number> directory under the repo",
    )
    parser.add_argument(
        "--changed-specs-from",
        default=None,
        help="Git ref/SHA used to discover changed specs/GH<number> packets for strict validation",
    )
    args = parser.parse_args()

    repo = Path(args.repo).resolve()
    errors: list[str] = []
    try:
        config = load_pack(repo)
        errors.extend(validate_required_files(repo))
        errors.extend(validate_tokens(repo))
        errors.extend(validate_json_schemas(repo))
        errors.extend(validate_state_graph(config))
        errors.extend(validate_labels(config))
        errors.extend(validate_action_policy(config))
        errors.extend(validate_skills_lock(repo))
        errors.extend(validate_template_parity(repo))
        selected_spec_dirs = select_spec_packet_dirs_for_validation(
            repo, args.spec_dir, all_specs=args.all_specs
        )
        selected_spec_dirs = merge_selected_spec_dirs(
            selected_spec_dirs,
            discover_changed_spec_packet_dirs(repo, args.changed_specs_from),
        )
        for spec_dir, strict in selected_spec_dirs:
            if not strict:
                continue
            errors.extend(validate_spec_packet(spec_dir))
    except SpecRailError as exc:
        errors.append(str(exc))

    if errors:
        print("SpecRail check failed")
        for error in errors:
            print(f"- {error}")
        return 1

    print("SpecRail check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
