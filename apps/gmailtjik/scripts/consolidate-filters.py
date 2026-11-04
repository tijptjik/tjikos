#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["requests>=2.32,<3"]
# ///
"""Plan and apply an audited, account-specific Gmail filter consolidation.

All snapshots, activity evidence and receipts stay in a private directory outside
Git. Planning only reads Gmail; applying changes filters, never existing mail.
"""
import argparse
import concurrent.futures
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import threading
import time
import urllib.parse
import urllib.request
import requests

API = "https://gmail.googleapis.com/gmail/v1/users/me/"
MAX_QUERY_BYTES = 1400


def private_write(path, value):
    path = Path(path)
    for parent in [path.parent.resolve(), *path.parent.resolve().parents]:
        if (parent / ".git").is_file() or (parent / ".git/HEAD").is_file():
            raise ValueError("Private filter data must be outside Git")
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_name(path.name + ".new")
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as stream:
        json.dump(value, stream, indent=2, ensure_ascii=False)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def action_key(action):
    return json.dumps({key: sorted(set(value)) if isinstance(value, list) else value
                       for key, value in action.items() if value not in ([], "", None)}, sort_keys=True)


def fingerprint(filters):
    return hashlib.sha256(json.dumps(sorted(filters, key=lambda f: f["id"]), sort_keys=True).encode()).hexdigest()


def criteria_query(criteria):
    supported = {"from", "to", "subject", "query", "negatedQuery", "hasAttachment", "excludeChats", "size", "sizeComparison"}
    if set(criteria) - supported:
        raise ValueError("Unsupported criteria: " + str(set(criteria) - supported))
    clauses = []
    for field in ("from", "to", "subject"):
        if criteria.get(field, "").strip():
            clauses.append(f"{field}:({criteria[field]})")
    for field, prefix in (("query", ""), ("negatedQuery", "-")):
        if criteria.get(field, "").strip():
            clauses.append(f"{prefix}({criteria[field]})")
    if criteria.get("hasAttachment"):
        clauses.append("has:attachment")
    if criteria.get("excludeChats"):
        clauses.append("-in:chats")
    if criteria.get("size", 0):
        comparison = criteria.get("sizeComparison")
        if comparison not in ("larger", "smaller"):
            raise ValueError("Unsupported size comparison")
        clauses.append(f"{comparison}:{criteria['size']}")
    if not clauses:
        raise ValueError("Empty criteria")
    return " ".join(clauses)


def sender_branches(criteria):
    """Split only explicit OR sender alternatives; leave other syntax intact."""
    sender = criteria.get("from", "").strip()
    value = sender
    if value.startswith("(") and value.endswith(")"):
        value = value[1:-1]
    alternatives = re.split(r"\s+OR\s+|\s+\|\s+", value)
    if len(alternatives) > 1 and all(re.fullmatch(r"[\w@.+*^/\-]+", part) for part in alternatives):
        return [dict(criteria, **{"from": part}) for part in alternatives]
    return [criteria]


def address_scoped(criteria):
    return bool(criteria.get("from") or criteria.get("to") or re.search(r"\b(?:from|to|list):", criteria.get("query", "")))


def activity_query(criteria):
    # An obsolete subject or body criterion must not make a still-active sender
    # look inactive. Check the address independently and preserve its full rule.
    for field in ("from", "to"):
        if criteria.get(field):
            value = criteria[field].strip()
            addresses = re.findall(r"[\w.+-]+@[\w.-]+\.[\w-]+", value)
            if len(addresses) == 1 and ("<" in value or " " in value or value.endswith(",")):
                value = addresses[0]
            return f"{field}:({value})"
    return criteria_query(criteria)


def compile_group(action, queries):
    filters = []
    chunk = []
    for query in sorted(set(queries)):
        branch = f"({query})"
        if len(branch.encode()) > MAX_QUERY_BYTES:
            raise ValueError("A condition exceeds the Gmail query limit")
        if chunk and len(" OR ".join([*chunk, branch]).encode()) > MAX_QUERY_BYTES:
            filters.append({"criteria": {"query": " OR ".join(chunk)}, "action": action})
            chunk = []
        chunk.append(branch)
    if chunk:
        filters.append({"criteria": {"query": " OR ".join(chunk)}, "action": action})
    return filters


class Gmail:
    def __init__(self, data):
        self.data = data
        self.lock = threading.Lock()
        self.local = threading.local()
        self.credential = json.loads((data / "account.json").read_text())

    def token(self):
        with self.lock:
            if self.credential.get("expires_at", 0) < time.time() + 90:
                credential = json.loads((self.data / "account.json").read_text())
                if credential.get("expires_at", 0) < time.time() + 90:
                    body = urllib.parse.urlencode({"grant_type": "refresh_token", **{key: credential[key] for key in ("refresh_token", "client_id", "client_secret")}}).encode()
                    with urllib.request.urlopen(urllib.request.Request("https://oauth2.googleapis.com/token", data=body), timeout=30) as response:
                        result = json.load(response)
                    # Use the access token in memory; leave the app's credentials untouched.
                    credential["access_token"] = result["access_token"]
                    credential["expires_at"] = time.time() + result.get("expires_in", 3600)
                self.credential = credential
            return self.credential["access_token"]

    def confirm_deleted(self, route):
        try:
            response = self.local.session.get(API + route,
                headers={"Authorization": "Bearer " + self.token()}, timeout=(10, 10))
            return response.status_code == 404
        except requests.RequestException:
            return False

    def request(self, route, method="GET", body=None, params=None):
        if not hasattr(self.local, "session"):
            self.local.session = requests.Session()
        attempts = 5 if method in ("GET", "DELETE") else 1
        for attempt in range(attempts):
            try:
                response = self.local.session.request(method, API + route, params=params, json=body,
                    headers={"Authorization": "Bearer " + self.token()}, timeout=(15, 15 if method == "DELETE" else 45))
            except requests.RequestException:
                if method == "DELETE" and self.confirm_deleted(route):
                    return None
                if attempt + 1 < attempts:
                    time.sleep(2 ** (attempt + 1))
                    continue
                raise RuntimeError(f"Gmail {method} connection outcome is uncertain; resume from the receipt") from None
            if method == "DELETE" and response.status_code == 404:
                return None
            if method == "DELETE" and response.status_code in (500, 502, 503, 504) and self.confirm_deleted(route):
                return None
            if response.status_code in (429, 500, 502, 503, 504) and attempt + 1 < attempts:
                time.sleep(max(2 ** (attempt + 1), 30 if response.status_code == 429 else 0))
                continue
            if not response.ok:
                raise RuntimeError(f"Gmail {method} failed with HTTP {response.status_code}")
            return response.json() if response.content else None

    def filters(self):
        return self.request("settings/filters").get("filter", [])


def plan(gmail, directory, since):
    if (directory / "receipt.json").exists():
        raise ValueError("Use a fresh directory after an applied or interrupted plan")
    original = gmail.filters()
    private_write(directory / "original.json", {"filter": original})
    cutoff = int(datetime.datetime.combine(since, datetime.time(), datetime.timezone(datetime.timedelta(hours=8))).timestamp())
    work = []
    for row in original:
        for criteria in sender_branches(row["criteria"]):
            query = criteria_query(criteria)
            scoped = address_scoped(criteria)
            check = "TRASH" not in row.get("action", {}).get("addLabelIds", []) and scoped and bool(row.get("action"))
            work.append({"source": row["id"], "criteria": criteria, "query": query, "action": row.get("action", {}), "check": check, "activity_query": activity_query(criteria)})
    queries = sorted({row["activity_query"] for row in work if row["check"]})
    evidence = {}
    print(f"Checking {len(queries)} distinct non-delete address conditions since {since.isoformat()}", flush=True)

    def check(query):
        search = f"({query}) after:{cutoff} -in:sent -in:drafts"
        result = gmail.request("messages", params={"q": search, "maxResults": 1, "includeSpamTrash": "true"})
        return query, {"query": search, "matched": bool(result.get("messages")), "checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}

    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        futures = [pool.submit(check, query) for query in queries]
        for future in concurrent.futures.as_completed(futures):
            query, result = future.result()
            evidence[query] = result
            private_write(directory / "activity.json", evidence)
            if len(evidence) % 25 == 0:
                print(f"Activity checked: {len(evidence)}/{len(queries)}", flush=True)
    groups = {}
    removed = []
    for row in work:
        if not row["action"]:
            removed.append(dict(row, reason="no action"))
        elif row["check"] and not evidence[row["activity_query"]]["matched"]:
            removed.append(dict(row, reason="no received match in two years"))
        else:
            groups.setdefault(action_key(row["action"]), []).append(row)
    replacements = []
    keep_ids = []
    for key, rows in groups.items():
        source_ids = {row["source"] for row in rows}
        source = [row for row in original if row["id"] in source_ids]
        unchanged = len(source) == 1 and len(rows) == 1 and rows[0]["criteria"] == source[0]["criteria"]
        # Keep single unchanged rules verbatim, including forwarding rules.
        if unchanged:
            keep_ids.extend(source_ids)
        else:
            replacements.extend(compile_group(json.loads(key), [row["query"] for row in rows]))
    delete_ids = sorted(row["id"] for row in original if row["id"] not in keep_ids)
    result = {"version": 1, "since": since.isoformat(), "original_fingerprint": fingerprint(original), "original": original, "replacements": replacements, "delete_ids": delete_ids, "keep_ids": sorted(keep_ids), "removed": removed,
              "summary": {"original_filters": len(original), "original_handling_groups": len({action_key(row.get('action', {})) for row in original}), "handling_groups": len(groups), "new_filters": len(replacements), "retained_filters": len(keep_ids), "final_filters": len(replacements) + len(keep_ids), "inactive_conditions_removed": sum(row['reason'] != 'no action' for row in removed), "no_action_filters_removed": len({row['source'] for row in removed if row['reason'] == 'no action'}), "activity_checks": len(queries)}}
    private_write(directory / "plan.json", result)
    print(json.dumps(result["summary"], indent=2), flush=True)


def apply(gmail, directory):
    plan = json.loads((directory / "plan.json").read_text())
    receipt_path = directory / "receipt.json"
    receipt = json.loads(receipt_path.read_text()) if receipt_path.exists() else {"created": [], "deleted": [], "pending_create": None}
    current = gmail.filters()
    if not receipt["created"] and not receipt["deleted"] and not receipt.get("pending_create"):
        if fingerprint(current) != plan["original_fingerprint"]:
            raise ValueError("Filters changed since planning; create a new plan")
    original_by_id = {row["id"]: row for row in plan["original"]}
    known_ids = set(original_by_id) | {row["id"] for row in receipt["created"]}
    # A timed-out create must be resolved explicitly before any deletion.
    if receipt.get("pending_create"):
        pending = receipt["pending_create"]
        matches = [row for row in current if row["id"] not in known_ids and row.get("criteria") == pending["criteria"] and action_key(row.get("action", {})) == action_key(pending["action"])]
        if len(matches) != 1:
            raise ValueError("Uncertain create outcome; inspect private receipt before resuming")
        receipt["created"].append(matches[0])
        receipt["pending_create"] = None
        private_write(receipt_path, receipt)
        known_ids.add(matches[0]["id"])
    if any(row["id"] not in known_ids for row in current):
        raise ValueError("Unexpected filters appeared; re-plan before modifying filters")
    for row in current:
        if row["id"] in original_by_id and row != original_by_id[row["id"]]:
            raise ValueError("An original filter changed; re-plan")
    for replacement in plan["replacements"][len(receipt["created"]):]:
        receipt["pending_create"] = replacement
        private_write(receipt_path, receipt)
        created = gmail.request("settings/filters", "POST", replacement)
        if created.get("criteria") != replacement["criteria"] or action_key(created.get("action", {})) != action_key(replacement["action"]):
            raise ValueError("Gmail returned a different replacement; originals preserved")
        receipt["created"].append(created)
        receipt["pending_create"] = None
        private_write(receipt_path, receipt)
    # Read back every replacement before removing any original filter.
    current = gmail.filters()
    by_id = {row["id"]: row for row in current}
    for row in receipt["created"]:
        if by_id.get(row["id"]) != row:
            raise ValueError("Replacement verification failed; originals preserved")
    remaining = [identity for identity in plan["delete_ids"] if identity not in receipt["deleted"]]

    # Serialize settings writes over one pooled connection. Record each completed
    # ID immediately; a slow or uncertain response is resolved by object read-back.
    for identity in remaining:
        receipt["pending_delete_ids"] = [identity]
        private_write(receipt_path, receipt)
        if identity in by_id:
            gmail.request("settings/filters/" + urllib.parse.quote(identity, safe=""), "DELETE")
            time.sleep(.25)
        receipt["deleted"].append(identity)
        receipt["pending_delete_ids"] = []
        receipt["delete_failures"] = {}
        private_write(receipt_path, receipt)
        if len(receipt["deleted"]) % 10 == 0:
            print(f"Old filters removed: {len(receipt['deleted'])}/{len(plan['delete_ids'])}", flush=True)
    final = gmail.filters()
    expected = sorted([*plan["keep_ids"], *(row["id"] for row in receipt["created"])])
    if sorted(row["id"] for row in final) != expected:
        raise ValueError("Final filter inventory differs from the plan")
    receipt["verified_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    receipt["summary"] = plan["summary"]
    private_write(directory / "final.json", {"filter": final})
    private_write(receipt_path, receipt)
    print(json.dumps({"verified": True, **plan["summary"]}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["plan", "apply"])
    parser.add_argument("directory", type=Path)
    parser.add_argument("--since", type=datetime.date.fromisoformat, default=datetime.date.today().replace(year=datetime.date.today().year - 2, day=min(datetime.date.today().day, 28) if datetime.date.today().month == 2 else datetime.date.today().day))
    parser.add_argument("--data", type=Path, default=Path(os.environ.get("GMAILTJIK_DATA_DIR", Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "gmailtjik")))
    args = parser.parse_args()
    if not args.directory.is_absolute():
        parser.error("Use an absolute private directory")
    args.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    # Serialize maintenance invocations and app filter saves; regular sync is unaffected.
    import fcntl
    with (args.data / "filters-maintenance.lock").open("a") as lock:
        os.chmod(lock.name, 0o600)
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        (plan if args.mode == "plan" else apply)(Gmail(args.data), args.directory, *([args.since] if args.mode == "plan" else []))
        if args.mode == "apply" and (args.data / "mail.sqlite").is_file():
            import sqlite3
            final = json.loads((args.directory / "final.json").read_text())["filter"]
            with sqlite3.connect("file:" + str(args.data / "mail.sqlite") + "?mode=rw", uri=True, timeout=10) as database:
                database.execute("INSERT INTO kv(key,value) VALUES('gmail_filters',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (json.dumps(final),))


if __name__ == "__main__":
    main()
