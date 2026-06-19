#!/usr/bin/env python3
"""Validate a sanitized Quest schema fixture contains the fields we depend on."""

from __future__ import annotations

import json
import sys
from pathlib import Path


REQUIRED_REWARD_KEYS = {"type", "sku_id", "orb_quantity", "premium_orb_quantity", "messages"}
REQUIRED_USER_STATUS_KEYS = {"claimed_at", "completed_at", "enrolled_at", "orb_quantity_claimed", "progress"}
REQUIRED_TASK_TYPES = {
    "WATCH_VIDEO",
    "WATCH_VIDEO_ON_MOBILE",
    "PLAY_ON_DESKTOP",
    "PLAY_ON_XBOX",
    "PLAY_ON_PLAYSTATION",
    "ACHIEVEMENT_IN_ACTIVITY",
}
REQUIRED_ENDPOINTS = {
    "GET /api/v9/quests/@me",
    "GET /api/v9/quests/decision",
    "GET /api/v9/quests/get-decisions",
}


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: validate-quest-schema-fixture.py <schema-fixture.json>")

    path = Path(sys.argv[1])
    data = json.loads(path.read_text(encoding="utf-8"))
    summary = data.get("quests_me_summary", {})
    endpoints = {
        f"{item.get('method')} {item.get('path')}"
        for item in data.get("quest_related_endpoints", [])
    }

    missing_endpoints = REQUIRED_ENDPOINTS - endpoints
    missing_reward = REQUIRED_REWARD_KEYS - set(summary.get("reward_keys", []))
    missing_status = REQUIRED_USER_STATUS_KEYS - set(summary.get("user_status_keys", []))
    missing_tasks = REQUIRED_TASK_TYPES - set(summary.get("task_types", {}).keys())

    errors = []
    if missing_endpoints:
        errors.append(f"missing endpoints: {sorted(missing_endpoints)}")
    if missing_reward:
        errors.append(f"missing reward keys: {sorted(missing_reward)}")
    if missing_status:
        errors.append(f"missing user_status keys: {sorted(missing_status)}")
    if missing_tasks:
        errors.append(f"missing task types: {sorted(missing_tasks)}")
    if summary.get("orb_rewards", 0) <= 0:
        errors.append("expected at least one Orbs reward")
    if summary.get("premium_orb_rewards", 0) <= 0:
        errors.append("expected at least one premium Orbs reward")
    if summary.get("claimed_orb_statuses", 0) <= 0:
        errors.append("expected at least one claimed Orbs status")

    if errors:
        raise SystemExit("\n".join(errors))

    print(f"Quest schema fixture OK: {path}")


if __name__ == "__main__":
    main()
