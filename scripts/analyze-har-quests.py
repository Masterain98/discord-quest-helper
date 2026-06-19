#!/usr/bin/env python3
"""Build a sanitized Quest API baseline from a Discord HAR file."""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any
from urllib.parse import urlparse


QUEST_CORE_PATHS = {"/api/v9/quests/@me"}
QUEST_DECISION_PATHS = {"/api/v9/quests/decision", "/api/v9/quests/get-decisions"}
SURROUNDING_MARKERS = (
    "/billing/subscriptions",
    "/virtual-currency/balance",
    "/entitlements",
    "/program-rewards",
    "/library",
    "/collectibles-marketing",
    "/content-inventory/users/@me",
    "/promotions",
)


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def captured_at_from_path(path: Path) -> str | None:
    match = re.search(r"(\d{4})_(\d{2})_(\d{2})", path.name)
    if not match:
        return None
    return "-".join(match.groups())


def query_keys(entry: dict[str, Any]) -> list[str]:
    keys: list[str] = []
    seen: set[str] = set()
    for item in entry.get("request", {}).get("queryString", []) or []:
        name = item.get("name")
        if name and name not in seen:
            seen.add(name)
            keys.append(name)
    return keys


def parse_body(entry: dict[str, Any]) -> Any | None:
    text = entry.get("response", {}).get("content", {}).get("text")
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None


def category_for_path(path: str) -> str | None:
    if path in QUEST_CORE_PATHS:
        return "quest_core"
    if path in QUEST_DECISION_PATHS:
        return "quest_decision_observed"
    if any(marker in path for marker in SURROUNDING_MARKERS):
        return "quest_surrounding"
    return None


def sanitize_path(path: str) -> str:
    return "/".join("{id}" if segment.isdigit() and len(segment) >= 12 else segment for segment in path.split("/"))


def response_keys(body: Any) -> list[str]:
    if isinstance(body, dict):
        return sorted(body.keys())
    if isinstance(body, list) and body and isinstance(body[0], dict):
        return sorted(body[0].keys())
    return []


def metadata_presence(value: Any) -> dict[str, Any]:
    if value is None:
        return {"present": False, "length": 0}
    if isinstance(value, str):
        return {"present": True, "length": len(value)}
    return {"present": True, "length": None}


def summarize_decision_body(body: Any) -> dict[str, Any]:
    if not isinstance(body, dict):
        return {}
    decisions = body.get("decisions")
    return {
        "keys": response_keys(body),
        "request_id_present": isinstance(body.get("request_id"), str),
        "quest_present": body.get("quest") is not None,
        "decisions_count": len(decisions) if isinstance(decisions, list) else None,
        "response_ttl_seconds": body.get("response_ttl_seconds"),
        "metadata_sealed": metadata_presence(body.get("metadata_sealed")),
        "traffic_metadata_raw": metadata_presence(body.get("traffic_metadata_raw")),
        "traffic_metadata_sealed": metadata_presence(body.get("traffic_metadata_sealed")),
    }


def summarize_quests_me(body: Any) -> dict[str, Any]:
    if not isinstance(body, dict):
        return {}

    quests = body.get("quests") if isinstance(body.get("quests"), list) else []
    excluded = body.get("excluded_quests") if isinstance(body.get("excluded_quests"), list) else []
    reward_fields: set[str] = set()
    user_status_fields: set[str] = set()
    config_fields: set[str] = set()
    task_types: Counter[str] = Counter()
    orb_rewards = 0
    premium_orb_rewards = 0
    multiplier_counts: Counter[str] = Counter()
    claimed_orb_statuses = 0

    for quest in quests:
        if not isinstance(quest, dict):
            continue

        config = quest.get("config")
        if isinstance(config, dict):
            config_fields.update(config.keys())
            task_config = config.get("task_config_v2") or config.get("task_config") or {}
            tasks = task_config.get("tasks") if isinstance(task_config, dict) else {}
            if isinstance(tasks, dict):
                for key, task in tasks.items():
                    task_type = key
                    if isinstance(task, dict) and isinstance(task.get("type"), str):
                        task_type = task["type"]
                    task_types[task_type] += 1

            rewards_config = config.get("rewards_config")
            rewards = rewards_config.get("rewards") if isinstance(rewards_config, dict) else []
            if isinstance(rewards, list):
                for reward in rewards:
                    if not isinstance(reward, dict):
                        continue
                    reward_fields.update(reward.keys())
                    is_orb = reward.get("type") == 4 or reward.get("orb_quantity") is not None
                    if is_orb:
                        orb_rewards += 1
                    base = reward.get("orb_quantity")
                    premium = reward.get("premium_orb_quantity")
                    if isinstance(base, (int, float)) and isinstance(premium, (int, float)):
                        premium_orb_rewards += 1
                        if base > 0:
                            multiplier_counts[f"{premium / base:.2f}"] += 1

        user_status = quest.get("user_status")
        if isinstance(user_status, dict):
            user_status_fields.update(user_status.keys())
            if user_status.get("orb_quantity_claimed") is not None:
                claimed_orb_statuses += 1

    return {
        "quests_count": len(quests),
        "excluded_quests_count": len(excluded),
        "quest_enrollment_blocked_until_present": body.get("quest_enrollment_blocked_until") is not None,
        "top_level_keys": response_keys(body),
        "quest_config_keys": sorted(config_fields),
        "reward_keys": sorted(reward_fields),
        "user_status_keys": sorted(user_status_fields),
        "task_types": dict(sorted(task_types.items())),
        "orb_rewards": orb_rewards,
        "premium_orb_rewards": premium_orb_rewards,
        "premium_multiplier_counts": dict(sorted(multiplier_counts.items())),
        "claimed_orb_statuses": claimed_orb_statuses,
    }


def build_baseline(har_path: Path) -> dict[str, Any]:
    har = load_json(har_path)
    entries = har.get("log", {}).get("entries", []) or []
    endpoint_map: dict[tuple[str, str], dict[str, Any]] = {}
    surrounding_counter: Counter[str] = Counter()
    quests_me_summary: dict[str, Any] | None = None

    for entry in entries:
        request = entry.get("request", {})
        url = request.get("url", "")
        parsed = urlparse(url)
        path = sanitize_path(parsed.path)
        category = category_for_path(path)
        if not category:
            continue

        method = request.get("method", "GET")
        key = (method, path)
        body = parse_body(entry)
        endpoint = endpoint_map.setdefault(
            key,
            {
                "method": method,
                "path": path,
                "category": category,
                "count": 0,
                "statuses": [],
                "query_keys": [],
                "response_keys": [],
            },
        )
        endpoint["count"] += 1
        endpoint["statuses"].append(entry.get("response", {}).get("status"))
        endpoint["query_keys"] = sorted(set(endpoint["query_keys"]) | set(query_keys(entry)))
        endpoint["response_keys"] = sorted(set(endpoint["response_keys"]) | set(response_keys(body)))

        if category == "quest_decision_observed":
            endpoint["decision_summary"] = summarize_decision_body(body)
        elif category == "quest_core" and path == "/api/v9/quests/@me":
            quests_me_summary = summarize_quests_me(body)
        elif category == "quest_surrounding":
            surrounding_counter[path] += 1

    return {
        "captured_at": captured_at_from_path(har_path),
        "source_file": har_path.name,
        "entries": len(entries),
        "quest_related_endpoints": list(endpoint_map.values()),
        "quests_me_summary": quests_me_summary or {},
        "surrounding_endpoint_counts": dict(sorted(surrounding_counter.items())),
        "redaction": {
            "headers_excluded": ["authorization", "cookie", "set-cookie", "x-super-properties"],
            "query_values_excluded": True,
            "metadata_sealed_raw_excluded": True,
            "user_and_guild_ids_excluded": True,
        },
    }


def compare_baselines(current: dict[str, Any], previous: dict[str, Any]) -> dict[str, Any]:
    def endpoint_key(item: dict[str, Any]) -> str:
        return f"{item.get('method')} {item.get('path')}"

    current_endpoints = {endpoint_key(item): item for item in current.get("quest_related_endpoints", [])}
    previous_endpoints = {endpoint_key(item): item for item in previous.get("quest_related_endpoints", [])}
    added = sorted(set(current_endpoints) - set(previous_endpoints))
    removed = sorted(set(previous_endpoints) - set(current_endpoints))

    query_key_changes: dict[str, Any] = {}
    response_key_changes: dict[str, Any] = {}
    for key in sorted(set(current_endpoints) & set(previous_endpoints)):
        current_query = set(current_endpoints[key].get("query_keys", []))
        previous_query = set(previous_endpoints[key].get("query_keys", []))
        if current_query != previous_query:
            query_key_changes[key] = {
                "added": sorted(current_query - previous_query),
                "removed": sorted(previous_query - current_query),
            }

        current_response = set(current_endpoints[key].get("response_keys", []))
        previous_response = set(previous_endpoints[key].get("response_keys", []))
        if current_response != previous_response:
            response_key_changes[key] = {
                "added": sorted(current_response - previous_response),
                "removed": sorted(previous_response - current_response),
            }

    return {
        "added_endpoints": added,
        "removed_endpoints": removed,
        "query_key_changes": query_key_changes,
        "response_key_changes": response_key_changes,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("har_path", type=Path)
    parser.add_argument("--compare", type=Path, help="Optional previous sanitized baseline JSON")
    args = parser.parse_args()

    baseline = build_baseline(args.har_path)
    if args.compare:
        baseline["diff"] = compare_baselines(baseline, load_json(args.compare))

    print(json.dumps(baseline, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
