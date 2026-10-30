#!/usr/bin/env python3
"""Runtjik: on-demand launcher backend. Stdlib except Gio desktop discovery."""
from __future__ import annotations

import argparse
import importlib.util
import hashlib
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import shlex
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import tomllib
from dataclasses import asdict, dataclass, field
from todo import Todo, OPTIONS as TODO_OPTIONS
from browser import Browser, DEFAULT_OPTIONS as BROWSER_OPTIONS
from words import Provider as Words, DEFAULT_OPTIONS, validate_options
from desktop_plugins import Clipboard, Recorder, Screenshots, DEFAULT_OPTIONS as DESKTOP_OPTIONS, validate_options as validate_desktop_options

BUNDLED_PROVIDERS = {provider.name: provider for provider in (Words, Clipboard, Recorder, Screenshots, Todo)}

OPTIONAL_PROVIDERS = {"Browser": Browser}

VERSION = "0.8.3"
API_VERSION = 1
INDEX_VERSION = 7
COMMAND_ICON = "org.gnome.Terminal"
DEFAULTS = {
    "ui": {"view": "grid", "columns": 5, "width": 720, "height": 600,
           "icon_size": 52, "font": "tijpset", "font_size": 14,
           "monitor": "focused", "max_results": 100, "keep_alive": True},
    "theme": {"background": "#191724", "surface": "#26233a", "tile": "#1f1d2e",
              "text": "#e0def4", "muted": "#908caa", "accent": "#ff53a6",
              "border": "#403a52", "icon_theme": "rose-punk-illustrated"},
    "index": {"desktop_apps": True, "path_commands": True, "refresh_seconds": 5},
    "launch": {"terminal": ["kitty", "--"], "isolate": True},
    "history": {"enabled": True},
    "plugins": {"enabled": list(BUNDLED_PROVIDERS)},
    "plugin_settings": {"Words": DEFAULT_OPTIONS, **DESKTOP_OPTIONS, "Browser": BROWSER_OPTIONS, "Todo": TODO_OPTIONS},
    "overrides": {},
}


def xdg(kind):
    fallback = {"config": ".config", "cache": ".cache", "state": ".local/state"}[kind]
    return Path(os.environ.get(f"XDG_{kind.upper()}_HOME", str(Path.home() / fallback))) / "runtjik"


def config_path():
    return Path(os.environ.get("RUNTJIK_CONFIG", str(xdg("config") / "runtjik.conf")))


def config(text=None):
    result = json.loads(json.dumps(DEFAULTS))
    path = config_path()
    raw = tomllib.loads(text) if text is not None else (tomllib.loads(path.read_text()) if path.exists() else {})
    for section, values in raw.items():
        if section not in result or not isinstance(values, dict):
            raise ValueError(f"Unknown config section: {section}")
        if section not in ("overrides", "plugin_settings") and set(values) - set(result[section]):
            raise ValueError(f"Unknown setting in [{section}]: {set(values) - set(result[section])}")
        if section == "plugin_settings":
            for name, options in values.items():
                if not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", name) or not isinstance(options, dict):
                    raise ValueError("Plugin settings must be named tables")
                result[section].setdefault(name, {}).update(options)
        else:
            result[section].update(values)
    for section in ("ui", "theme", "index", "launch", "history", "plugins"):
        for key, default in DEFAULTS[section].items():
            if type(result[section][key]) is not type(default):
                raise ValueError(f"{section}.{key} must be {type(default).__name__}")
    for key, lower, upper in [("columns", 2, 10), ("width", 420, 1800), ("height", 320, 1400),
                              ("icon_size", 24, 128), ("font_size", 10, 24), ("max_results", 10, 500)]:
        if not lower <= result["ui"][key] <= upper:
            raise ValueError(f"ui.{key} must be between {lower} and {upper}")
    if result["ui"]["view"] not in ("grid", "list"):
        raise ValueError("ui.view must be grid or list")
    if not 1 <= result["index"]["refresh_seconds"] <= 3600:
        raise ValueError("index.refresh_seconds must be between 1 and 3600")
    if not result["launch"]["terminal"] or not all(isinstance(s, str) for s in result["launch"]["terminal"]):
        raise ValueError("launch.terminal must be a nonempty argv array")
    for key, override in result["overrides"].items():
        if not isinstance(override, dict) or set(override) - {"argv", "window_classes"}:
            raise ValueError(f"Invalid override for {key}")
        for name, value in override.items():
            if not isinstance(value, list) or not value or not all(isinstance(v, str) for v in value):
                raise ValueError(f"overrides.{key}.{name} must be a nonempty string array")
    if not all(isinstance(v, str) and re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", v) for v in result["plugins"]["enabled"]):
        raise ValueError("plugins.enabled must contain provider module names")
    validate_options(result["plugin_settings"]["Words"])
    validate_desktop_options(result["plugin_settings"])
    if set(result["plugin_settings"]["Browser"]) - set(BROWSER_OPTIONS):
        raise ValueError("Unknown Browser setting")
    columns = result["plugin_settings"]["Browser"]["columns"]
    if type(columns) is not int or not 1 <= columns <= 6:
        raise ValueError("Browser.columns must be an integer from 1 to 6")
    return result


FEATURED_SETTINGS = {
    "plugins.enabled": "Enabled bundled or installed provider module names",
    "ui.keep_alive": "Keep the launcher warm in memory while hidden",
    "ui.view": "Default view: grid or list",
    "ui.columns": "Columns in the icon grid (2–10)",
    "ui.width": "Window width in pixels (420–1800)",
    "ui.height": "Window height in pixels (320–1400)",
    "ui.icon_size": "Icon size in pixels (24–128)",
    "ui.font_size": "Text size in pixels (10–24)",
    "index.path_commands": "Include previously used commands from Fish history",
    "history.enabled": "Record new launch and switch events",
}
SHORTCUTS = [
    ("Super+Escape", "Toggle Runtjik (desktop binding, on key press)"),
    ("Tab / Shift+Tab", "Next / previous result"),
    ("Up / Down", "Previous / next row"),
    ("Left / Right", "Previous / next result in the grid; Ctrl+Left / Ctrl+Right also works in lists"),
    ("Enter / click", "Switch to a running application, or launch if none are running"),
    ("Ctrl+Enter", "Close a browser tab; trash a capture; otherwise close the selected app"),
    ("Ctrl+Z", "Undo the most recent capture trash action while browsing capture history"),
    ("Ctrl+Up / Ctrl+Down", "Previous / next launcher command; Down past newest restores the draft"),
    ("Shift+Enter / Shift+click", "Launch a new application instance; browser Read later integration is pending"),
    ("Ctrl+L", "Toggle grid / list for this session"),
    ("Esc", "Return from a command or instance picker to general search; otherwise close"),
    ("Drag the header or frame", "Move within a monitor; release on another monitor to move there"),
]


def featured_settings(settings):
    rows = dict(FEATURED_SETTINGS)
    for name, options in settings["plugin_settings"].items():
        for key, value in options.items():
            if re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", key) and type(value) in (str, bool, int, float, list):
                rows[f"plugin_settings.{name}.{key}"] = f"{name} plugin · {key.replace('_', ' ')}"
    rows.update({
        "plugin_settings.Words.define_sources": "Sources: free_dictionary, wiktionary, urban_dictionary (optional slang)",
        "plugin_settings.Words.synonym_sources": "Sources: datamuse, free_dictionary",
    })
    return rows


def get_setting(settings, key):
    for part in key.split("."):
        settings = settings[part]
    return settings


def setting_value(key, text):
    if key not in featured_settings(config()):
        raise ValueError("This setting is not exposed by /config; edit runtjik.conf directly")
    try:
        value = tomllib.loads("value = " + text)["value"]
    except tomllib.TOMLDecodeError:
        if key == "ui.view" and text in ("grid", "list"):
            value = text
        else:
            raise ValueError("Use a TOML value, for example 6, true, or \"list\"") from None
    return value


def setting_text(source, key, value):
    """Update a featured value, preserving surrounding text and comments."""
    section, field = key.rsplit(".", 1)
    literal = json.dumps(value, ensure_ascii=False)
    lines = source.splitlines(keepends=True)
    begin = end = None
    for i, line in enumerate(lines):
        header = re.match(r"^\s*\[([^\]]+)\]\s*(?:#.*)?$", line.strip())
        if header:
            if begin is not None:
                end = i
                break
            if header[1].strip() == section:
                begin = i + 1
    if begin is None:
        updated = source.rstrip("\n") + f"\n\n[{section}]\n{field} = {literal}\n"
    else:
        end = len(lines) if end is None else end
        for i in range(begin, end):
            match = re.match(r"^(\s*" + re.escape(field) + r"\s*=\s*)(.*?)(\r?\n)?$", lines[i])
            if match:
                comment = ""
                for token in re.finditer(r'''"(?:\\.|[^"\\])*"|'[^']*'|(#.*)''', match[2]):
                    if token[1]:
                        comment = "  " + token[1]
                        break
                last = i + 1
                while True:
                    try:
                        tomllib.loads("".join(lines[i:last]))
                        break
                    except tomllib.TOMLDecodeError:
                        if last >= end:
                            raise ValueError("Cannot safely edit this setting layout") from None
                        last += 1
                lines[i:last] = [match[1] + literal + comment + (match[3] or "\n")]
                break
        else:
            if end and not lines[end - 1].endswith("\n"):
                lines[end - 1] += "\n"
            lines.insert(end, f"{field} = {literal}\n")
        updated = "".join(lines)
    config(updated)  # Reject unsupported layouts/types/ranges before touching disk.
    return updated


def save_setting(key, text):
    value = setting_value(key, text)
    path = config_path().resolve()
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with path.with_name(path.name + ".lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        source = path.read_text() if path.exists() else ""
        updated = setting_text(source, key, value)
        replace_config(path, source, updated)
    return config(updated)


def replace_config(path, source, updated):
    fd, name = tempfile.mkstemp(prefix=".runtjik-conf-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as stream:
            stream.write(updated)
            stream.flush()
            os.fsync(stream.fileno())
        if path.exists():
            os.chmod(name, path.stat().st_mode & 0o777)
        # Do not silently overwrite an external editor's intervening save.
        if (path.read_text() if path.exists() else "") != source:
            raise ValueError("Config changed while saving; retry the command")
        os.replace(name, path)
    finally:
        Path(name).unlink(missing_ok=True)


def migrate_config(enable_bundled=False):
    """Explicit, backed-up migration; never run automatically during installation."""
    path = config_path().resolve()
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with path.with_name(path.name + ".lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        source = path.read_text() if path.exists() else ""
        raw = tomllib.loads(source)
        settings = config(source)
        updated = source
        if enable_bundled:
            enabled = list(dict.fromkeys([*settings["plugins"]["enabled"], *BUNDLED_PROVIDERS]))
            if raw.get("plugins", {}).get("enabled") != enabled:
                updated = setting_text(updated, "plugins.enabled", enabled)
        for name, defaults in DEFAULTS["plugin_settings"].items():
            for key, value in defaults.items():
                if key not in raw.get("plugin_settings", {}).get(name, {}):
                    updated = setting_text(updated, f"plugin_settings.{name}.{key}", value)
        if updated == source:
            return {"changed": False, "path": str(path)}
        backup = None
        if path.exists():
            backup = path.with_name(path.name + ".pre-plugins-" + str(time.time_ns()))
            fd = os.open(backup, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, "w") as stream:
                stream.write(source)
                stream.flush()
                os.fsync(stream.fileno())
        replace_config(path, source, updated)
        return {"changed": True, "path": str(path), "backup": str(backup) if backup else None}


@dataclass
class Entry:
    id: str
    name: str
    subtitle: str = ""
    icon: str = "application-x-executable"
    provider: str = "apps"
    keywords: str = ""
    data: dict = field(default_factory=dict)


FRECENCY_HALF_LIFE = 7 * 24 * 60 * 60


def decay(score, elapsed):
    return score * 2 ** (-max(0, elapsed) / FRECENCY_HALF_LIFE)


class Store:
    """Events and derived key/value counters commit atomically, never log queries."""
    def __init__(self, path=None):
        path = Path(path or xdg("state") / "history.sqlite3")
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.db = sqlite3.connect(path, timeout=5)
        os.chmod(path, 0o600)
        self.db.executescript("""
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS events (
                seq INTEGER PRIMARY KEY, time REAL NOT NULL, entry_id TEXT NOT NULL,
                action TEXT NOT NULL, success INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS counts (
                key TEXT PRIMARY KEY, value INTEGER NOT NULL, last REAL NOT NULL);
            CREATE TABLE IF NOT EXISTS command_queries (
                seq INTEGER PRIMARY KEY AUTOINCREMENT, query TEXT NOT NULL UNIQUE);
            CREATE TABLE IF NOT EXISTS close_requests (
                entry_id TEXT NOT NULL, pid INTEGER NOT NULL, identity TEXT NOT NULL,
                stage INTEGER NOT NULL, PRIMARY KEY(entry_id,pid));
        """)
        # One-time backfill. Normal opens and queries only read the small cache.
        if not self.db.execute("SELECT 1 FROM sqlite_master WHERE name='frecency'").fetchone():
            with self.db:
                self.db.execute("BEGIN IMMEDIATE")
                # Another launcher/CLI process may have migrated while we waited.
                if not self.db.execute("SELECT 1 FROM sqlite_master WHERE name='frecency'").fetchone():
                    self.db.execute("CREATE TABLE frecency (entry_id TEXT PRIMARY KEY, score REAL NOT NULL, last REAL NOT NULL)")
                    self._rebuild_frecency()

    def remember_command(self, query):
        if not isinstance(query, str) or not query.strip() or not query.startswith(('!', '/', '?')):
            return
        with self.db:
            self.db.execute('INSERT OR REPLACE INTO command_queries(query) VALUES(?)', (query,))
            self.db.execute('DELETE FROM command_queries WHERE seq NOT IN (SELECT seq FROM command_queries ORDER BY seq DESC LIMIT 100)')

    def command_history(self):
        return [row[0] for row in self.db.execute('SELECT query FROM command_queries ORDER BY seq')]

    def _add_frecency(self, entry_id, timestamp):
        previous = self.db.execute("SELECT score,last FROM frecency WHERE entry_id=?", (entry_id,)).fetchone()
        reference = max(timestamp, previous[1]) if previous else timestamp
        score = decay(previous[0], reference - previous[1]) if previous else 0
        score += decay(1, reference - timestamp)
        self.db.execute("INSERT OR REPLACE INTO frecency VALUES(?,?,?)", (entry_id, score, reference))

    def _rebuild_frecency(self):
        self.db.execute("DELETE FROM frecency")
        for entry_id, timestamp in self.db.execute(
                "SELECT entry_id,time FROM events WHERE success=1 AND action IN ('launch','switch') ORDER BY time,seq"):
            self._add_frecency(entry_id, timestamp)

    def frecency(self, now=None):
        now = time.time() if now is None else now
        return {entry_id: decay(score, now - last) for entry_id, score, last in
                self.db.execute("SELECT entry_id,score,last FROM frecency")}

    def record(self, entry_id, action, success=True):
        now = time.time()
        with self.db:
            self.db.execute("INSERT INTO events(time,entry_id,action,success) VALUES(?,?,?,?)",
                            (now, entry_id, action, int(success)))
            if success:
                self.db.execute("""INSERT INTO counts VALUES(?,1,?) ON CONFLICT(key)
                    DO UPDATE SET value=value+1,last=excluded.last""", (action + ":" + entry_id, now))
                if action in ("launch", "switch"):
                    self._add_frecency(entry_id, now)

    def counts(self):
        return {k: {"count": v, "last": t} for k, v, t in self.db.execute("SELECT key,value,last FROM counts")}

    def pending_closes(self, entry_id):
        pending = {}
        with self.db:
            for pid, identity, stage in self.db.execute(
                    "SELECT pid,identity,stage FROM close_requests WHERE entry_id=?", (entry_id,)).fetchall():
                if process_identity(pid) == identity:
                    pending[pid] = (identity, stage)
                else:
                    self.db.execute("DELETE FROM close_requests WHERE entry_id=? AND pid=?", (entry_id, pid))
        return pending

    def remember_close(self, entry_id, pid, identity, stage):
        with self.db:
            self.db.execute("INSERT OR REPLACE INTO close_requests VALUES(?,?,?,?)", (entry_id, pid, identity, stage))

    def rebuild(self):
        with self.db:
            self.db.execute("DELETE FROM counts")
            self.db.execute("""INSERT INTO counts SELECT action || ':' || entry_id,COUNT(*),MAX(time)
                FROM events WHERE success=1 GROUP BY action,entry_id""")
            self._rebuild_frecency()


def app_dirs():
    roots = [os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share"))]
    roots += os.environ.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share").split(":")
    roots += [str(Path.home() / ".local/share/flatpak/exports/share"),
              "/var/lib/flatpak/exports/share", "/var/lib/snapd/desktop"]
    return list(dict.fromkeys(Path(r) / "applications" for r in roots if r))


def path_dirs():
    # Ignore empty/relative PATH elements: the launcher must not depend on cwd.
    return list(dict.fromkeys(Path(p) for p in os.environ.get("PATH", "").split(":") if p.startswith("/")))



def fish_history_path():
    name = os.environ.get("fish_history", "fish")
    if not name or not re.fullmatch(r"[\w.-]+", name):
        return None
    return Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share"))) / "fish" / (name + "_history")


def literal_word(source):
    # Decode literal shell words only. Never expand variables or substitutions.
    if any(char in source for char in "$(){}*?\n"):
        return None
    try:
        words = shlex.split(source)
        return words[0] if len(words) == 1 else None
    except ValueError:
        return None


def history_head(words):
    """Unwrap common launch prefixes, discarding every recorded argument."""
    wrappers = {"sudo", "doas", "env", "nohup", "nice", "mullvad-exclude"}
    value_options = {"sudo": {"-u", "-g", "-h", "-p", "-C", "-T", "-R", "-D", "--user", "--group", "--host", "--prompt", "--chdir", "--chroot"},
                     "doas": {"-u", "-C"}, "env": {"-u", "--unset", "-C", "--chdir"}, "nice": {"-n", "--adjustment"}}
    while words:
        name = words.pop(0)
        if not name:
            return None
        if name not in wrappers:
            return Path(name).name if not name.startswith("-") else None
        while words:
            word = words[0]
            if not word:
                return None
            if word == "--":
                words.pop(0)
                break
            if name == "env" and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", word):
                words.pop(0)
            elif word in value_options.get(name, set()):
                del words[:2]
            elif word.startswith("-"):
                # Shell/string execution is not a literal executable invocation.
                if word in ("-s", "-i", "-S", "--split-string", "--shell", "--login"):
                    return None
                words.pop(0)
            else:
                break
    return None


def fish_commands():
    path = fish_history_path()
    parser = shutil.which("fish_indent")
    if path is None or not parser:
        return set()
    try:
        with path.open(errors="replace") as history:
            records = [re.sub(r"\\([\\n])", lambda m: "\n" if m[1] == "n" else "\\", line[7:].rstrip("\n"))
                       for line in history if line.startswith("- cmd: ")]
    except OSError:
        return set()
    # Fish's own parser distinguishes command positions from arguments, quoted
    # semicolons, redirections, pipelines and control flow. It never executes code.
    # Deduplicate before parsing; neither source text nor arguments enter the cache.
    records = list(dict.fromkeys(records))
    if not records:
        return set()
    try:
        parsed = subprocess.run([parser, "--dump-parse-tree"], input="\n".join(records) + "\n",
                                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, timeout=8)
    except (OSError, subprocess.TimeoutExpired):
        return set()
    result, words, level = set(), [], None
    def finish():
        if words:
            # Keep wrapper executables themselves as well as their literal target.
            if words[0]:
                result.add(Path(words[0]).name)
            target = history_head(words.copy())
            if target:
                result.add(target)
    for line in parsed.stderr.splitlines():
        depth = len(line) - len(line.lstrip("! "))
        node = line.lstrip("! ")
        if level is not None and depth <= level:
            finish()
            words, level = [], None
        if node == "decorated_statement":
            level = depth
        elif level is not None and (node.startswith("string: '") or node.startswith("argument: '")) and node.endswith("'"):
            words.append(literal_word(node.split(": ", 1)[1][1:-1]))
    finish()
    return result


def signature(settings):
    values = [settings["index"], os.environ.get("PATH", ""), os.environ.get("XDG_CURRENT_DESKTOP", "")]
    for directory in (app_dirs() if settings["index"]["desktop_apps"] else []) + (path_dirs() if settings["index"]["path_commands"] else []):
        try:
            files = directory.rglob("*.desktop") if directory in app_dirs() else directory.iterdir()
            values.append((str(directory), directory.stat().st_mtime_ns))
            for p in sorted(files):
                try:
                    s = p.stat()
                    values.append((str(p), s.st_mtime_ns, s.st_mode, s.st_size))
                except OSError:
                    continue
        except OSError:
            continue
    if settings["index"]["path_commands"]:
        history = fish_history_path()
        try:
            stat = history.stat() if history else None
            values.append((str(history), stat.st_mtime_ns, stat.st_size, stat.st_ino) if stat else (None,))
        except OSError:
            values.append((str(history), None))
    return values


# Presentation aliases do not alter the installed desktop files or packages.
APP_NAMES = {
    "Zoom.desktop": "Zoom", "us.zoom.Zoom.desktop": "Zoom",
    "gimp.desktop": "GIMP", "org.gimp.GIMP.desktop": "GIMP",
    "obsidian-appimage.desktop": "Markdown Viewer",
}
HIDDEN_APPS = {"scrcpy-console.desktop", "uuctl.desktop", "rygel-preferences.desktop"}
HIDDEN_COMMANDS = {"hamr", "hamr-gtk", "hamr-daemon"}
GUI_WRAPPERS = {"bash", "sh", "dash", "zsh", "fish", "env", "flatpak", "snap", "uwsm",
                "systemd-run", "mullvad-exclude", "python", "python3", "node", "bun", "java", "wine"}
APP_ICONS = {"Zoom.desktop": "zoom", "us.zoom.Zoom.desktop": "zoom"}
COMMAND_ICONS = {"notjik": "notjik", "bartjik": "bartjik", "blueman-manager": "blueman",
                 "zoom": "zoom", "dnf": "fedora-media-writer", "dnf5": "fedora-media-writer",
                 "fish": "fish"}
GUI_COMMANDS = {"notjik", "bartjik", "blueman-manager", "zoom"}


def retired_entry(entry):
    executable = Path(entry.data.get("executable", "")).name
    desktop = entry.data.get("desktop_id", "").casefold()
    return (entry.name.casefold() in {"hamr", "hamr launcher"}
            or entry.id.removeprefix('commands:') in HIDDEN_COMMANDS
            or executable in HIDDEN_COMMANDS
            or desktop in {"hamr.desktop", "hamr-gtk.desktop", "com.hamr.Hamr.desktop".casefold()})

# Only migrate ranking history for retired desktop IDs; never hide live entries.
HISTORY_ALIASES = {
    "dev.zed.Zed.desktop": ["dev.zed.desktop"],
    "brave-browser.desktop": ["com.brave.Browser.desktop"],
    "google-chrome-canary.desktop": ["com.google.Chrome.canary.desktop"],
    "org.telegram.desktop._3e485da34fc040f9218e3891ecde1e6c.desktop": ["org.telegram.desktop"],
    "FFPWA-01KCGW1793YX2BA145Z4NWQ2XY.desktop": ["FFPWA-01KC3V7SZSJ8PMK49J7QBTP2FK.desktop"],
    "FFPWA-01KVFSZ3Z9JKBJMMTW9DENGR0N.desktop": ["FFPWA-01KVFQER39ASXCZ26D7724G54G.desktop"],
}


def decorate_commands(entries):
    applications = {}
    for entry in entries:
        if entry.provider == "apps":
            executable = entry.data.get("executable", "")
            try:
                arguments = shlex.split(entry.data.get("commandline") or executable)[1:]
            except ValueError:
                continue
            if (executable and Path(executable).name not in GUI_WRAPPERS
                    and not entry.data.get("terminal")
                    and not any(not arg.startswith('%') for arg in arguments)):
                resolved = shutil.which(executable)
                if resolved:
                    applications.setdefault(str(Path(resolved).resolve()), []).append(entry)
                applications.setdefault(Path(executable).name, []).append(entry)
    for entry in entries:
        if entry.provider != "commands":
            continue
        command = entry.name
        argv = entry.data.get("argv", [])
        entry.icon = COMMAND_ICONS.get(command, COMMAND_ICON)
        entry.data.pop("gui", None)
        entry.data.pop("classes", None)
        matches = applications.get(str(Path(argv[0]).resolve()), []) if argv else []
        matches = matches or applications.get(command, [])
        unique = {match.id: match for match in matches}
        application = next(iter(unique.values())) if len(unique) == 1 and command not in GUI_WRAPPERS else None
        if application:
            entry.icon = application.icon
            entry.data["gui"] = not application.data.get("terminal", False)
            entry.data["classes"] = application.data.get("classes", [])
        if command in COMMAND_ICONS:
            entry.icon = COMMAND_ICONS[command]
            if command in GUI_COMMANDS:
                entry.data["gui"] = True
    return entries


class Apps:
    name = "apps"

    def index(self, settings):
        if not settings["index"]["desktop_apps"]:
            return []
        import gi
        gi.require_version("Gio", "2.0")
        from gi.repository import Gio
        seen, result = set(), []
        for directory in app_dirs():
            if not directory.is_dir():
                continue
            for path in sorted(directory.rglob("*.desktop")):
                desktop_id = str(path.relative_to(directory)).replace("/", "-")
                if desktop_id in seen:
                    continue
                seen.add(desktop_id)  # Hidden overrides mask lower precedence entries.
                if desktop_id in HIDDEN_APPS:
                    continue
                try:
                    app = Gio.DesktopAppInfo.new_from_filename(str(path))
                except (TypeError, ValueError):
                    continue  # Invalid entries and failed TryExec are not runnable.
                if app is None or app.get_is_hidden() or not app.should_show():
                    continue
                executable = app.get_executable() or ""
                if not executable and not app.get_boolean("DBusActivatable"):
                    continue
                if executable and not shutil.which(executable):
                    continue
                icon = app.get_icon()
                wm_class = app.get_startup_wm_class()
                result.append(Entry("apps:" + desktop_id, APP_NAMES.get(desktop_id, app.get_display_name()),
                                    app.get_generic_name() or "Application", APP_ICONS.get(desktop_id, icon.to_string() if icon else "application-x-executable"),
                                    keywords=" ".join(app.get_keywords() or []) + " " + (app.get_description() or ""),
                                    data={"path": str(path), "desktop_id": desktop_id,
                                          "executable": executable, "commandline": app.get_commandline() or "",
                                          "terminal": app.get_boolean("Terminal"),
                                          "classes": list(dict.fromkeys([wm_class, desktop_id.removesuffix(".desktop")])) if wm_class else [desktop_id.removesuffix(".desktop"),
                                                                                  Path(executable).name]}))
        for entry in result:
            entry.data["aliases"] = [entry.id] + ["apps:" + name for name in HISTORY_ALIASES.get(entry.data["desktop_id"], [])]
        return [entry for entry in result if not retired_entry(entry)]

    def activate(self, entry, settings):
        override = settings["overrides"].get(entry.id, {}).get("argv")
        argv = override or ["gio", "launch", entry.data["path"]]
        dispatch(argv, settings)


class Commands:
    name = "commands"

    def index(self, settings):
        if not settings["index"]["path_commands"]:
            return []
        allowed = fish_commands()
        result, seen = [], set()
        for directory in path_dirs():
            try:
                for path in sorted(directory.iterdir()):
                    if path.name in allowed and path.name not in HIDDEN_COMMANDS and path.name not in seen and path.is_file() and os.access(path, os.X_OK):
                        seen.add(path.name)
                        result.append(Entry("commands:" + path.name, path.name, str(path.parent),
                                            COMMAND_ICON, "commands", data={"argv": [str(path)]}))
            except OSError:
                continue
        return result

    def activate(self, entry, settings):
        argv = settings["overrides"].get(entry.id, {}).get("argv") or (
            entry.data["argv"] if entry.data.get("gui") else settings["launch"]["terminal"] + entry.data["argv"])
        dispatch(argv, settings)


def dispatch(argv, settings):
    if not argv or not shutil.which(argv[0]):
        raise ValueError(f"Executable unavailable: {argv[0] if argv else '(empty)'}")
    if settings["launch"]["isolate"] and shutil.which("systemd-run") and os.environ.get("DBUS_SESSION_BUS_ADDRESS"):
        # Service, not scope: launched apps survive closing the on-demand UI.
        session_env = ["--setenv=" + key + "=" + os.environ[key] for key in
                       ("WAYLAND_DISPLAY", "DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "XDG_CURRENT_DESKTOP",
                        "XDG_SESSION_TYPE", "XDG_RUNTIME_DIR", "PATH", "XDG_DATA_HOME", "XDG_DATA_DIRS") if key in os.environ]
        proc = subprocess.run(["systemd-run", "--user", "--collect", "--quiet", "--property=Type=exec", "--property=ExitType=cgroup", *session_env,
                               "--working-directory=" + str(Path.home()), "--", *argv], capture_output=True, text=True, timeout=8)
        if proc.returncode:
            raise RuntimeError(proc.stderr.strip() or "Application launch failed")
    else:
        subprocess.Popen(argv, start_new_session=True, cwd=Path.home(), stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def windows():
    if not os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        return []
    result = subprocess.run(["hyprctl", "-j", "clients"], capture_output=True, text=True, timeout=3)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "Unable to read Hyprland windows")
    return json.loads(result.stdout)


def matched_windows(entry, clients, settings):
    classes = settings["overrides"].get(entry.id, {}).get("window_classes", entry.data.get("classes", []))
    tokens = {s.casefold() for s in classes if s}
    def matches(window):
        names = {str(window.get(k, "")).casefold() for k in ("class", "initialClass")}
        return window.get("mapped", True) and any(name == token or name.startswith(token + "._")
                                                  for name in names for token in tokens)
    return sorted([w for w in clients if matches(w)], key=lambda w: w.get("focusHistoryID", 9999))


def lua_string(value):
    return '"' + ''.join('\\' + c if c in '\\"' else f'\\{ord(c):03d}' if ord(c) < 32 else c for c in str(value)) + '"'


def focus_commands(client, monitors, lua=True):
    address = client.get("address", "")
    if not re.fullmatch(r"0x[0-9a-fA-F]+", address):
        raise ValueError("Invalid compositor window address")
    monitor = next((m for m in monitors if m["id"] == client.get("monitor")), None)
    workspace = client.get("workspace", {}).get("name", "")
    commands = []
    if monitor:
        # Dismiss overlays elsewhere without hiding a target inside one.
        for other in monitors:
            special = other.get("specialWorkspace", {}).get("name", "")
            if not special or (other["id"] == monitor["id"] and special == workspace):
                continue
            commands.append('hl.dsp.focus({ monitor = ' + lua_string(other["name"]) + ' })' if lua else 'focusmonitor ' + other["name"])
            name = special.removeprefix("special:") if special != "special" else ""
            commands.append('hl.dsp.workspace.toggle_special(' + lua_string(name) + ')' if lua else 'togglespecialworkspace ' + name)
        commands.append('hl.dsp.focus({ monitor = ' + lua_string(monitor["name"]) + ' })' if lua else 'focusmonitor ' + monitor["name"])
        if (workspace == "special" or workspace.startswith("special:")) and monitor.get("specialWorkspace", {}).get("name") != workspace:
            name = workspace.removeprefix("special:") if workspace != "special" else ""
            commands.append('hl.dsp.workspace.toggle_special(' + lua_string(name) + ')' if lua else 'togglespecialworkspace ' + name)
    commands.append('hl.dsp.focus({ window = ' + lua_string('address:' + address) + ' })' if lua else 'focuswindow address:' + address)
    return [["hyprctl", "dispatch", value] if lua else ["hyprctl", "dispatch", *value.split(" ", 1)] for value in commands]


def focus_window(client):
    probe = subprocess.run(["hyprctl", "eval", "return"], capture_output=True, text=True, timeout=3)
    lua = probe.returncode == 0 and probe.stdout.strip() == "ok"
    monitors = json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], text=True, timeout=3))
    for command in focus_commands(client, monitors, lua):
        completed = subprocess.run(command, capture_output=True, text=True, timeout=3)
        if completed.returncode or completed.stdout.strip() != "ok":
            raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "Window switch failed")


def process_identity(pid):
    """Bind retries to an owned process in this boot, never a reused PID."""
    if not isinstance(pid, int) or pid <= 1 or pid == os.getpid():
        return None
    try:
        directory = Path('/proc') / str(pid)
        if directory.stat().st_uid != os.getuid():
            return None
        fields = (directory / 'stat').read_text().rsplit(')', 1)[1].split()
        if fields[0] in ('Z', 'X'):
            return None
        boot = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
        return boot + ':' + fields[19]
    except (OSError, IndexError):
        return None


def signal_process(pid, identity, signum):
    if process_identity(pid) != identity:
        return False
    if not hasattr(os, 'pidfd_open') or not hasattr(signal, 'pidfd_send_signal'):
        # Some standalone Python builds omit pidfds; the desktop Python has them.
        if os.path.samefile(sys.executable, '/usr/bin/python3'):
            raise RuntimeError('The system Python needs pidfd support to retry closing an app')
        completed = subprocess.run(['/usr/bin/python3', '-B', str(Path(__file__).resolve()),
                                    '_signal-close', str(pid), identity, str(int(signum))],
                                   capture_output=True, text=True, timeout=3)
        if completed.returncode:
            raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or 'Unable to signal app')
        result = json.loads(completed.stdout)
        if type(result) is not bool:
            raise RuntimeError('Invalid process signal response')
        return result
    try:
        descriptor = os.pidfd_open(pid)
        try:
            if process_identity(pid) != identity:
                return False
            signal.pidfd_send_signal(descriptor, signum)
        finally:
            os.close(descriptor)
    except ProcessLookupError:
        return False
    return True


def close_windows(clients):
    addresses = [client.get('address', '') for client in clients]
    if any(not re.fullmatch(r'0x[0-9a-fA-F]+', address) for address in addresses):
        raise ValueError('Invalid compositor window address')
    probe = subprocess.run(['hyprctl', 'eval', 'return'], capture_output=True, text=True, timeout=3)
    lua = probe.returncode == 0 and probe.stdout.strip() == 'ok'
    for address in addresses:
        command = (['hyprctl', 'dispatch', 'hl.dsp.window.close({window=' + lua_string('address:' + address) + '})']
                   if lua else ['hyprctl', 'dispatch', 'closewindow', 'address:' + address])
        completed = subprocess.run(command, capture_output=True, text=True, timeout=3)
        if completed.returncode or completed.stdout.strip() != 'ok':
            raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or 'Window close request failed')


def match_score(query, entry):
    if not query:
        return 0
    name = entry.name.casefold()
    hay = name + " " + entry.keywords.casefold() + " " + entry.subtitle.casefold()
    score = 0
    for word in query.casefold().split():
        if word == name:
            score += 1000
        elif name.startswith(word):
            score += 500
        elif word in name:
            score += 250
        elif word in hay:
            score += 80
        else:
            iterator = iter(name)
            if not all(any(c == ch for c in iterator) for ch in word):
                return None
            score += 20
    return score


class Icons:
    """Resolve the configured theme and rasterize SVGs with librsvg, not Qt SVG Tiny."""
    def __init__(self, settings):
        self.theme_name = settings["theme"]["icon_theme"]
        self.size = settings["ui"]["icon_size"] * 2
        self.theme = None
        self.cache = {}
        self.records = {}
        self.dirty = False
        environment = [self.theme_name, self.size, str(Path.home()), os.environ.get("XDG_DATA_HOME"), os.environ.get("XDG_DATA_DIRS")]
        key = hashlib.sha256(json.dumps(environment).encode()).hexdigest()
        self.manifest = xdg("cache") / "icons" / ("lookup-" + key + ".json")
        try:
            saved = json.loads(self.manifest.read_text())
            if (saved["schema"] == 1 and isinstance(saved["records"], dict) and isinstance(saved["watched"], dict)
                    and saved["watched"] and all(self.stamp(Path(path)) == stamp for path, stamp in saved["watched"].items())):
                self.records = saved["records"]
        except (OSError, ValueError, KeyError, TypeError):
            pass

    @staticmethod
    def stamp(path):
        try:
            value = path.stat()
            return [value.st_mtime_ns, value.st_ctime_ns, value.st_size, value.st_ino]
        except OSError:
            return None

    def initialize(self):
        if self.theme is not None:
            return
        import gi
        gi.require_version("Gtk", "3.0")
        from gi.repository import Gtk, GdkPixbuf
        self.Gtk, self.Pixbuf = Gtk, GdkPixbuf.Pixbuf
        self.theme = Gtk.IconTheme.new()
        self.theme.set_custom_theme(self.theme_name)

    def flush(self):
        if not self.dirty or self.theme is None:
            return
        # Directory metadata detects new icons and changed lookup precedence;
        # source-file metadata below detects edits to existing artwork.
        watched, visited = {}, set()
        for root in self.theme.get_search_path():
            path = Path(root)
            watched[str(path)] = self.stamp(path)
            for directory, children, files in os.walk(path, followlinks=True):
                node = Path(directory)
                stamp = self.stamp(node)
                watched[str(node)] = stamp
                if stamp is None:
                    children[:] = []
                    continue
                identity = (node.stat().st_dev, stamp[3])
                if identity in visited:
                    children[:] = []
                    continue
                visited.add(identity)
                if "index.theme" in files:
                    watched[str(node / "index.theme")] = self.stamp(node / "index.theme")
        self.manifest.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        temporary = self.manifest.with_suffix(f".{os.getpid()}.tmp")
        try:
            temporary.write_text(json.dumps({"schema": 1, "watched": watched, "records": self.records}))
            os.chmod(temporary, 0o600)
            temporary.replace(self.manifest)
            self.dirty = False
        finally:
            temporary.unlink(missing_ok=True)

    def resolve(self, name):
        if name in self.cache:
            return self.cache[name]
        record = self.records.get(name)
        if isinstance(record, dict):
            try:
                stamp = self.stamp(Path(record["source"]))
                if stamp is not None and stamp == record["stamp"] and Path(record["resolved"]).is_file():
                    self.cache[name] = record["resolved"]
                    return record["resolved"]
            except (KeyError, TypeError):
                pass
        self.initialize()
        path = Path(name) if name.startswith("/") else None
        if path is None:
            for candidate in name.split() + ["application-x-executable"]:
                info = self.theme.lookup_icon(candidate, self.size, self.Gtk.IconLookupFlags.FORCE_SIZE)
                if info:
                    path = Path(info.get_filename())
                    break
        result = name
        if path and path.exists():
            result = str(path)
            if path.suffix.lower() == ".svg":
                key = hashlib.sha256(f"{path}:{path.stat().st_mtime_ns}:{self.size}".encode()).hexdigest()
                target = xdg("cache") / "icons" / (key + ".png")
                if not target.exists():
                    target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                    temporary = target.with_suffix(f".{os.getpid()}.tmp")
                    try:
                        image = self.Pixbuf.new_from_file_at_scale(str(path), self.size, self.size, True)
                        image.savev(str(temporary), "png", [], [])
                        temporary.replace(target)
                    finally:
                        temporary.unlink(missing_ok=True)
                result = str(target)
        if path and path.is_file():
            self.records[name] = {"source": str(path), "stamp": self.stamp(path), "resolved": result}
            self.dirty = True
        self.cache[name] = result
        return result


class Launcher:
    def __init__(self, demo=False):
        self.demo = demo
        self.closing_windows = {}
        self.settings = config()
        self._icons = None
        self.store = None if demo else Store()
        self.providers = {p.name: p for p in (Apps(), Commands())}
        self.warnings = []
        for name in self.settings["plugins"]["enabled"]:
            try:
                if name in BUNDLED_PROVIDERS or name in OPTIONAL_PROVIDERS:
                    provider = (BUNDLED_PROVIDERS | OPTIONAL_PROVIDERS)[name](demo=self.demo)
                else:
                    path = xdg("config") / "plugins" / (name + ".py")
                    spec = importlib.util.spec_from_file_location("runtjik_ext_" + name, path)
                    module = importlib.util.module_from_spec(spec)
                    spec.loader.exec_module(module)
                    if module.API_VERSION != API_VERSION:
                        raise ValueError("Unsupported API version")
                    provider = module.Provider()
                if provider.name in self.providers or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", provider.name):
                    raise ValueError("Duplicate or invalid provider name")
                self.providers[provider.name] = provider
            except Exception as exc:
                self.warnings.append(f"{name}: {exc}")
        self.plugin_commands = {}
        reserved = {"/run", "/processes", "/refresh", "/pin", "/grid", "/list", "/stats", "/config", "/bindings", "/shortcuts", "/running"}
        for name, provider in self.providers.items():
            for command, description in getattr(provider, "commands", {}).items():
                if not re.fullmatch(r"/[a-z][a-z0-9_-]*", command) or command in reserved or command in self.plugin_commands:
                    self.warnings.append(f"{name}: duplicate or invalid command {command}")
                    continue
                self.plugin_commands[command] = (name, description)
        self.plugin_aliases = {}
        for name, provider in self.providers.items():
            for alias, canonical in getattr(provider, "aliases", {}).items():
                if (not re.fullmatch(r"/[a-z][a-z0-9_-]*", alias) or alias in reserved or alias in self.plugin_commands
                        or canonical not in self.plugin_commands or self.plugin_commands[canonical][0] != name):
                    self.warnings.append(f"{name}: invalid command alias {alias}")
                    continue
                self.plugin_commands[alias] = self.plugin_commands[canonical]
                self.plugin_aliases[alias] = canonical
        self.entries = []
        self.last_signature = None
        self.checked = 0.0
        self.current = {}
        self.reset_ranking()
        self.refresh()

    def reset_ranking(self):
        self.ranking_snapshot = None
        self.ranked_queries = {}
        self.last_query = None

        self.page_query = None
        self.page_ranked = []
    @property
    def icons(self):
        if self._icons is None:
            self._icons = Icons(self.settings)
        return self._icons

    def refresh(self, force=False):
        if force:
            self._icons = None
        if self.demo:
            self.entries = demo_entries()
            return
        sig = signature(self.settings)
        cache = xdg("cache") / "index.json"
        if not force and not self.entries:
            try:
                cached = json.loads(cache.read_text())
                if cached["version"] == VERSION and cached.get("schema") == INDEX_VERSION and cached["signature"] == json.loads(json.dumps(sig)):
                    self.entries = [Entry(**item) for item in cached["entries"]]
            except (OSError, ValueError, KeyError, TypeError):
                pass
        if force or not self.entries or (self.last_signature is not None and sig != self.last_signature):
            entries = []
            for name in ("apps", "commands"):
                entries.extend(self.providers[name].index(self.settings))
            self.entries = decorate_commands(entries)
            cache.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            # Per-process filename + atomic replace makes simultaneous CLI refresh safe.
            temporary = cache.with_suffix(f".{os.getpid()}.tmp")
            temporary.write_text(json.dumps({"version": VERSION, "schema": INDEX_VERSION, "signature": sig, "entries": [asdict(e) for e in entries]}))
            os.chmod(temporary, 0o600)
            temporary.replace(cache)
        # Normalize cached discovery too, including retired apps and stale GUI matches.
        self.entries = decorate_commands([entry for entry in self.entries if not retired_entry(entry)])
        self.last_signature = sig
        self.checked = time.monotonic()

    def control_icon(self, name):
        # Use the app artwork loader, including full SVG rendering and theme selection.
        try:
            return self.icons.resolve(name)
        except Exception:
            return name  # Artwork must not hide usable launcher commands.

    def query(self, query="", offset=0, limit=None):
        if type(offset) is not int or offset < 0:
            raise ValueError('Invalid result offset')
        if limit is None:
            limit = self.settings['ui']['max_results']
        if type(limit) is not int or limit < 1:
            raise ValueError('Invalid result page size')
        if query == "" and self.last_query not in (None, ""):
            self.reset_ranking()
        self.last_query = query
        if time.monotonic() - self.checked >= self.settings["index"]["refresh_seconds"]:
            self.refresh()
        display_query = query
        if query.startswith('!') and not query.startswith('!!'):
            query = '/' + query[1:].lstrip()
        run_mode = query == "/run" or query.startswith("/run ") or query.startswith('!!')
        running_mode = query.startswith("?") or query == "/processes" or query.startswith("/processes ")
        search_query = query[2:].lstrip() if query.startswith('!!') else query[4:].lstrip() if run_mode else query
        if running_mode:
            search_query = query[1:].lstrip() if query.startswith("?") else query.partition(" ")[2].lstrip()
        command = query.split(maxsplit=1)[0] if query.strip() else ""
        if command != '/tabs' and 'Browser' in self.providers:
            self.providers['Browser'].window_scope = None
        if command in self.plugin_commands:
            name, _ = self.plugin_commands[command]
            try:
                if getattr(self.providers[name], 'mode', '') == 'app':
                    return self.payload([], query=display_query, app_mode=name,
                                        app_state=self.providers[name].state({}, self.settings))
                entries = [value if isinstance(value, Entry) else Entry(**value)
                           for value in self.providers[name].query(query, self.settings)]
                if any(e.provider != name or not e.id.startswith(name + ":") for e in entries):
                    raise ValueError("Provider result ID must be namespaced")
                self.current = {e.id: e for e in entries}
                rows = [{**{k: v for k, v in asdict(e).items() if k not in ("data", "keywords")},
                         "icon": self.control_icon(e.icon), "favicon": e.data.get("favicon", ""), "hide_launcher": bool(e.data.get("capture")), "keep_open": bool(e.data.get("keep_open")), "thumbnail": e.data.get("thumbnail", ""), "empty_state": bool(e.data.get("empty_state")), "disabled": bool(e.data.get("disabled")), "count": 0, "windows": 0} for e in entries[offset:offset + limit]]
                scope = getattr(self.providers[name], 'window_scope', None)
                return self.payload(rows, query=display_query, browser_tab_scope=bool(scope),
                                    message=('Tabs in ' + scope[2] + ' · Esc returns to search') if scope else '',
                                    pending=bool(getattr(self.providers[name], "pending", False)), total=len(entries), offset=offset,
                                    next_offset=offset + len(rows), has_more=offset + len(rows) < len(entries))
            except Exception as exc:
                return self.payload([], query=display_query, message=f"{name}: {exc}",
                                    app_mode=name if getattr(self.providers[name], "mode", "") == "app" else "")
        if query.startswith("/") and not run_mode and not running_mode:
            if query.strip() in ("/shortcuts", "/bindings"):
                return self.payload([{"id": "/shortcuts/" + str(i), "name": keys, "subtitle": description,
                                      "provider": "internal", "icon": self.control_icon("runtjik-shortcuts"), "count": 0,
                                      "windows": 0, "disabled": True} for i, (keys, description) in enumerate(SHORTCUTS)], query=display_query)
            if query == "/config" or query.startswith("/config "):
                parts = query.split(maxsplit=2)
                if len(parts) == 3:
                    try:
                        value = setting_value(parts[1], parts[2])
                        setting_text(config_path().read_text() if config_path().exists() else "", parts[1], value)
                        rows = [{"id": query, "name": "Set " + parts[1] + " = " + json.dumps(value),
                                 "subtitle": "Enter saves runtjik.conf and applies this setting", "disabled": False}]
                    except (ValueError, OSError) as exc:
                        rows = [{"id": "/config/error", "name": "Cannot save this value", "subtitle": str(exc), "disabled": True}]
                else:
                    rows = []
                    for key, description in featured_settings(self.settings).items():
                        if len(parts) == 2 and not key.startswith(parts[1]):
                            continue
                        rows.append({"id": "/config " + key, "name": key + " = " + json.dumps(get_setting(self.settings, key)),
                                     "subtitle": description, "disabled": False})
                return self.payload([{**row, "provider": "internal", "icon": self.control_icon("runtjik-config"), "count": 0, "windows": 0} for row in rows], query=display_query)
            commands = [("/processes", "Search applications with running windows", "runtjik-stats"),
                        ("/run", "Search previously used terminal commands", COMMAND_ICON),
                        ("/refresh", "Rebuild the application and command index", "runtjik-refresh"),
                        ("/pin", "Toggle keeping the launcher open on outside clicks", "runtjik-pin"),
                        ("/grid", "Show applications as an icon grid", "runtjik-grid"),
                        ("/list", "Show applications as a list", "runtjik-list"),
                        ("/stats", "Show cumulative launches and window switches", "runtjik-stats"),
                        ("/config", "View and change featured settings", "runtjik-config"),
                        ("/bindings", "Show keyboard shortcuts and dragging", "runtjik-shortcuts")]
            commands.extend((command, description, getattr(self.providers[name], "icon", "accessories-dictionary"))
                            for command, (name, description) in self.plugin_commands.items() if command not in self.plugin_aliases)
            results = [{"id": name, "name": name + ("  ·  " + ", ".join(alias for alias, canonical in self.plugin_aliases.items() if canonical == name) if name in self.plugin_aliases.values() else "") + ({'/processes':'  ·  ?', '/run':'  ·  !!', '/bindings':'  ·  /shortcuts'}.get(name,'')), "subtitle": text, "icon": self.control_icon(icon), "provider": "internal", "count": 0, "windows": 0}
                       for name, text, icon in sorted(commands, key=lambda row: row[0].casefold())
                       if name.startswith(query) or any(alias.startswith(query) and canonical == name for alias, canonical in self.plugin_aliases.items()) or (name == '/bindings' and '/shortcuts'.startswith(query))]
            return self.payload(results, query=display_query)
        counts = self.store.counts() if self.store else {}
        if self.ranking_snapshot is None:
            self.ranking_snapshot = self.store.frecency() if self.store else {}
        frecency = self.ranking_snapshot
        candidates = [entry for entry in self.entries if entry.provider == ("commands" if run_mode else "apps")]
        for name, provider in self.providers.items():
            if name in ("apps", "commands") or run_mode or running_mode:
                continue
            try:
                for value in provider.query(query, self.settings):
                    entry = value if isinstance(value, Entry) else Entry(**value)
                    if entry.provider != name or not entry.id.startswith(name + ":"):
                        raise ValueError("Provider result ID must be namespaced")
                    candidates.append(entry)
            except Exception as exc:
                warning = f"{name}: {exc}"
                if warning not in self.warnings:
                    self.warnings.append(warning)
        try:
            clients = demo_windows() if self.demo else windows()
        except (OSError, RuntimeError, ValueError, subprocess.TimeoutExpired) as exc:
            clients = []
            if str(exc) not in self.warnings:
                self.warnings.append(str(exc))
        live_addresses = {client['address'] for client in clients}
        self.closing_windows = {key: addresses & live_addresses for key, addresses in self.closing_windows.items()
                                if addresses & live_addresses}
        ranked = []
        for entry in candidates:
            if running_mode and not matched_windows(entry, clients, self.settings):
                continue
            score = match_score(search_query, entry)
            if score is not None:
                aliases = entry.data.get("aliases", [entry.id])
                history = [counts.get("launch:" + key, {"count": 0, "last": 0}) for key in aliases]
                stat = {"count": sum(v["count"] for v in history), "last": max(v["last"] for v in history)}
                ranked.append((entry, score, stat))
        ranked.sort(key=lambda row: (-row[1], -sum(frecency.get(key, 0) for key in row[0].data.get("aliases", [row[0].id])), row[0].provider == "commands", row[0].name.casefold(), row[0].id))
        # Keep already displayed rows in place; append discoveries until reset.
        previous_order = self.ranked_queries.get(query, [])
        positions = {entry_id: i for i, entry_id in enumerate(previous_order)}
        ranked.sort(key=lambda row: positions.get(row[0].id, len(positions)))
        self.ranked_queries[query] = [row[0].id for row in ranked]
        if offset and self.page_query == query:
            ranked = self.page_ranked
        else:
            self.page_query, self.page_ranked = query, ranked
        page = ranked[offset:offset+limit]
        if not offset:
            self.current = {}
        self.current.update({entry.id: entry for entry, _, _ in page})
        results = []
        for entry, _, stat in page:
            row = {k: v for k, v in asdict(entry).items() if k not in ("data", "keywords")}
            try:
                row["icon"] = self.icons.resolve(entry.icon)
            except Exception:
                pass  # Broken optional artwork must not hide a runnable application.
            row.update(favicon=entry.data.get("favicon", ""), keep_open=bool(entry.data.get("keep_open")), count=stat["count"], windows=len(matched_windows(entry, clients, self.settings)))
            results.append(row)
        if self._icons is not None:
            try:
                self._icons.flush()
            except OSError:
                pass  # A read-only cache must not prevent launching.
        next_offset = offset + len(results)
        return self.payload(results, total=len(ranked), query=display_query, offset=offset,
                            next_offset=next_offset, has_more=next_offset < len(ranked), closing=list(self.closing_windows))

    def payload(self, results, **extra):
        return {"kind": "results", "version": VERSION, "results": results, "config": self.settings, "indexed": len(self.entries),
                "warnings": self.warnings, "demo": self.demo, **extra}

    def close_requests_for(self, entry):
        classes = self.settings['overrides'].get(entry.id, {}).get('window_classes', entry.data.get('classes', []))
        if entry.provider == 'commands' and not entry.data.get('gui') and not classes:
            # Old wrapper-to-GUI matches must not escalate against an unrelated app.
            return {}
        return self.store.pending_closes(entry.id)

    def close_app(self, entry, address='', pending=None):
        if self.demo:
            return {'kind': 'activated', 'action': 'close', 'demo': True,
                    'message': 'Demo: graceful close request for ' + entry.name}
        retry_only = pending is not None
        pending = self.close_requests_for(entry) if pending is None else pending
        clients = [] if retry_only else matched_windows(entry, windows(), self.settings)
        if address:
            clients = [client for client in clients if client['address'] == address]
            if not clients:
                return {'kind': 'message', 'message': 'That window has already closed'}
            pending = {pid: value for pid, value in pending.items() if any(c.get('pid') == pid for c in clients)}
        if not clients and not pending:
            return {'kind': 'message', 'message': 'No running window or pending close request for ' + entry.name}
        groups, untracked = {}, []
        for client in clients:
            pid = client.get('pid')
            identity = process_identity(pid)
            if identity is None:
                untracked.append(client)
            else:
                groups.setdefault(pid, (identity, []))[1].append(client)
        stages = set()
        if untracked:
            close_windows(untracked)
            stages.add(0)
        for pid in dict.fromkeys([*groups, *pending]):
            if pid in groups:
                identity, client_windows = groups[pid]
            else:
                identity, client_windows = pending[pid][0], []
            previous = pending.get(pid)
            stage = min(previous[1] + 1, 2) if previous and previous[0] == identity else 0
            if stage:
                if not signal_process(pid, identity, signal.SIGTERM if stage == 1 else signal.SIGKILL):
                    continue
            else:
                close_windows(client_windows)
            self.store.remember_close(entry.id, pid, identity, stage)
            stages.add(stage)
        if not stages:
            return {'kind': 'message', 'message': 'The previous process has already exited; launch again to reopen ' + entry.name}
        if self.settings['history']['enabled']:
            self.store.record(entry.id, 'close')
        self.closing_windows.setdefault(entry.id, set()).update(client['address'] for client in clients)
        labels = ('graceful close', 'SIGTERM', 'SIGKILL')
        return {'kind': 'activated', 'action': 'close', 'demo': False, 'entry': entry.id,
                'message': 'Close requested for ' + entry.name + ': ' + ', '.join(labels[stage] for stage in sorted(stages))}

    def activate(self, entry_id, mode="launch", address="", prepare=False):
        if mode == 'close' and entry_id.startswith('/'):
            return {'kind': 'message', 'message': 'Ctrl+Enter closes applications, not launcher commands'}
        if entry_id in ("/run", "/processes", "?") or entry_id in self.plugin_commands:
            return {"kind": "edit", "text": entry_id + " "}
        if entry_id == "/pin":
            return {"kind": "pin"}
        if entry_id in ("/shortcuts", "/bindings", "/config"):
            return {"kind": "edit", "text": entry_id}
        if entry_id.startswith("/config "):
            parts = entry_id.split(maxsplit=2)
            if len(parts) == 2 and parts[1] in featured_settings(self.settings):
                return {"kind": "edit", "text": entry_id + " " + json.dumps(get_setting(self.settings, parts[1])), "select_value": True}
            if len(parts) != 3:
                raise ValueError("Use /config section.key value")
            if self.demo:
                return {"kind": "message", "message": "Demo mode does not change your config"}
            self.settings = save_setting(parts[1], parts[2])
            replacement = Launcher(self.demo)
            if self.store:
                self.store.db.close()
            self.__dict__.update(replacement.__dict__)
            return {**self.query("/config"), "edit_query": "/config", "apply_config": True,
                    "message": "Saved " + parts[1] + " in runtjik.conf"}
        if entry_id.startswith("/shortcuts/") or entry_id == "/config/error":
            return {"kind": "message", "message": "Type a command or press Escape to close"}
        if entry_id == "/refresh":
            self.refresh(True)
            if self.store and self.settings["history"]["enabled"]:
                self.store.record("index", "refresh")
            return {**self.query(), "reset_query": True, "message": f"Index rebuilt · {len(self.entries)} entries"}
        if entry_id in ("/grid", "/list"):
            return {"kind": "view", "view": entry_id[1:]}
        if entry_id == "/stats":
            counts = self.store.counts() if self.store else {}
            launches = sum(v["count"] for k, v in counts.items() if k.startswith("launch:"))
            switches = sum(v["count"] for k, v in counts.items() if k.startswith("switch:"))
            return {"kind": "message", "message": f"{launches} launches · {switches} window switches · {len(self.entries)} indexed entries"}
        entry = self.current.get(entry_id) or next((e for e in self.entries if e.id == entry_id), None)
        if entry is None:
            raise ValueError("Entry is no longer available; refresh the index")
        if mode not in ("launch", "switch", "close"):
            raise ValueError("Unknown action")
        if entry.provider == 'Browser' and mode == 'switch':
            if entry.data.get('tab') is None:
                return self.providers['Browser'].open_tabs(entry)
            return {'kind': 'message', 'message': 'Read later is moving to Tasktjik; integration is not connected yet. Nothing was closed.'}
        if mode == 'close':
            if entry.provider == 'Browser':
                action = self.providers['Browser'].activate(entry, self.settings, mode)
                return {**self.query(self.last_query or '/tabs'), 'message': action['message']}
            if entry.provider in ('Screenshots', 'Recorder') and entry.data.get('file'):
                action = self.providers[entry.provider].activate(entry, self.settings, mode)
                return {**self.query(self.last_query or action.get('refresh_query', '')), 'message': action.get('message', '')}
            if entry.provider not in ('apps', 'commands'):
                return {'kind': 'message', 'message': 'Ctrl+Enter is available for applications and commands with matching windows'}
            return self.close_app(entry, address)
        if mode == 'launch' and entry.provider in ('apps', 'commands') and not self.demo:
            pending = self.close_requests_for(entry)
            if pending:
                return self.close_app(entry, pending=pending)
        if entry.provider not in ("apps", "commands"):
            if not self.demo or entry.provider in BUNDLED_PROVIDERS or entry.provider in OPTIONAL_PROVIDERS:
                action = self.providers[entry.provider].activate(entry, self.settings, mode)
                if isinstance(action, dict) and "refresh_query" in action:
                    return self.query(action["refresh_query"])
                if isinstance(action, dict):
                    return action
        elif mode == "switch":
            clients = matched_windows(entry, demo_windows() if self.demo else windows(), self.settings)
            if not clients:
                if prepare:
                    return {"kind": "launch", "entry": entry_id}
                return self.activate(entry_id, "launch")
            if not address and (prepare or len(clients) > 1):
                return {"kind": "windows", "entry": entry_id, "auto_select": prepare and len(clients) == 1, "results": [
                    {"id": w["address"], "name": w.get("title") or entry.name,
                     "subtitle": "Workspace " + str(w.get("workspace", {}).get("name", "")),
                     "icon": self.icons.resolve(entry.icon), "provider": "windows", "windows": 0, "count": 0} for w in clients]}
            target = next((w for w in clients if w["address"] == address), None) if address else clients[0]
            if target is None:
                raise ValueError("That window has closed; select another instance")
            if not self.demo:
                focus_window(target)
        elif not self.demo:
            try:
                self.providers[entry.provider].activate(entry, self.settings)
            except Exception:
                if self.settings["history"]["enabled"]:
                    self.store.record(entry.id, "launch", False)
                raise
        if self.store and self.settings["history"]["enabled"]:
            self.store.record(entry.id, mode)
        return {"kind": "activated", "demo": self.demo}


def demo_entries():
    rows = [("Files", "File manager", "org.gnome.Nautilus"), ("Firefox", "Web browser", "firefox"),
            ("Zen", "Web browser", "zen-browser"), ("Terminal", "Terminal emulator", "kitty"),
            ("Obsidian", "Notes and ideas", "obsidian"), ("Signal", "Private messenger", "signal"),
            ("Telegram", "Messaging", "telegram"), ("Music", "Music player", "spotify"),
            ("Calculator", "Quick calculations", "org.gnome.Calculator"), ("Calendar", "Your schedule", "org.gnome.Calendar"),
            ("Maps", "Explore places", "org.gnome.Maps"), ("Settings", "System preferences", "org.gnome.Settings"),
            ("System Monitor", "System resources", "org.gnome.SystemMonitor"), ("Photos", "Photo library", "org.gnome.Photos"),
            ("Text Editor", "Write something", "org.gnome.TextEditor")]
    return [Entry("apps:" + icon + ".desktop", name, subtitle, icon, data={"classes": [icon]}) for name, subtitle, icon in rows]


def demo_windows():
    return [{"address": "0xdemo1", "class": "firefox", "title": "Design notes — Firefox", "workspace": {"name": "1"}},
            {"address": "0xdemo2", "class": "firefox", "title": "Documentation — Firefox", "workspace": {"name": "2"}}]


def main():
    if sys.argv[1:2] == ['_signal-close']:
        if len(sys.argv) != 5 or int(sys.argv[4]) not in (signal.SIGTERM, signal.SIGKILL):
            raise ValueError('Invalid close retry request')
        print(json.dumps(signal_process(int(sys.argv[2]), sys.argv[3], int(sys.argv[4]))))
        return
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["serve", "refresh", "stats", "rebuild-counts", "check", "version", "migrate-config"])
    parser.add_argument("--demo", action="store_true")
    parser.add_argument("--enable-bundled-plugins", action="store_true")
    args = parser.parse_args()
    if args.command == "migrate-config":
        print(json.dumps(migrate_config(args.enable_bundled_plugins)))
        return
    if args.command == "version":
        print(VERSION)
        return
    if args.command == "check":
        config()
        print("Configuration valid")
        return
    if args.command in ("stats", "rebuild-counts"):
        store = Store()
        if args.command == "rebuild-counts":
            store.rebuild()
        print(json.dumps(store.counts(), indent=2))
        return
    launcher = Launcher(args.demo)
    if args.command == "refresh":
        launcher.refresh(True)
        print(json.dumps({"indexed": len(launcher.entries)}))
        return
    def emit(value):
        print(json.dumps(value), flush=True)
    emit({**launcher.query(), 'command_history': launcher.store.command_history() if launcher.store and launcher.settings['history']['enabled'] else []})
    for line in sys.stdin:
        request = {}
        try:
            request = json.loads(line)
            if request["op"] == "remember-command":
                if launcher.store and launcher.settings['history']['enabled']:
                    launcher.store.remember_command(request.get('query', ''))
                continue
            if request["op"] == "query":
                value = launcher.query(request.get("query", ""), request.get('offset',0), request.get('limit'))
            elif request["op"] == "app-action":
                provider = launcher.providers.get(request.get('provider'))
                if provider is None or getattr(provider, 'mode', '') != 'app':
                    raise ValueError('App provider is unavailable')
                value = {'kind': 'app-state', 'app_state': provider.state(request, launcher.settings)}
            elif request["op"] == "resume":
                # Reopening observes external TOML edits, including provider changes.
                if config() != launcher.settings:
                    replacement = Launcher(args.demo)
                    if launcher.store:
                        launcher.store.db.close()
                    launcher = replacement
                launcher.reset_ranking()
                launcher.refresh()
                value = {**launcher.query(), "apply_config": True, "command_history": launcher.store.command_history() if launcher.store and launcher.settings["history"]["enabled"] else []}
            elif request["op"] == "undo-trash":
                from desktop_plugins import trash_action
                action = {"message": "Demo mode leaves Trash unchanged"} if launcher.demo else trash_action('undo')
                value = {**launcher.query(request.get('query', '')), 'message': action['message']}
            elif request["op"] == "activate":
                value = launcher.activate(request["id"], request.get("mode", "launch"), request.get("address", ""), request.get("prepare", False))
            else:
                raise ValueError("Unknown operation")
            emit({**value, "serial": request.get("serial", 0)})
        except Exception as exc:
            emit({"kind": "error", "message": str(exc), "serial": request.get("serial", 0)})


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(json.dumps({"kind": "error", "message": str(exc)}), flush=True)
        sys.exit(1)
