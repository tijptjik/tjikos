"""Versioned JSON workflow configuration. Commands are argv arrays, never shell text."""

from __future__ import annotations

import json
import math
import os
from pathlib import Path
import re
from string import Template

from .runtime import UpdateError


def fields(value, allowed, where):
    if not isinstance(value, dict):
        raise UpdateError(f"{where}: expected an object")
    unknown = value.keys() - allowed
    if unknown:
        raise UpdateError(f"{where}: unknown fields: {', '.join(sorted(unknown))}")


def string(value, where):
    if not isinstance(value, str) or not value or "\x00" in value:
        raise UpdateError(f"{where}: expected a nonempty string without NUL")
    return value


def presentation(value, where):
    if "section" in value:
        string(value["section"], f"{where}.section")
    if "banner" in value:
        banner = value["banner"]
        fields(banner, {"title", "url"}, f"{where}.banner")
        string(banner.get("title"), f"{where}.banner.title")
        string(banner.get("url"), f"{where}.banner.url")


def environment(value, where):
    if not isinstance(value, dict):
        raise UpdateError(f"{where}: expected an environment object")
    for key, item in value.items():
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key):
            raise UpdateError(f"{where}: invalid variable {key!r}")
        if not isinstance(item, str) or "\x00" in item:
            raise UpdateError(f"{where}.{key}: expected a string without NUL")


def expand(value, directory, env):
    try:
        value = Template(value.replace("{config_dir}", str(directory))).substitute(env)
    except (KeyError, ValueError) as error:
        raise UpdateError(f"configuration expansion failed: {error}") from error
    return (
        str(Path(env.get("HOME", str(Path.home()))) / value[2:])
        if value.startswith("~/")
        else value
    )


def load_config(filename):
    path = Path(filename).expanduser().resolve()
    try:
        config = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise UpdateError(f"cannot load configuration {path}: {error}") from error
    fields(config, {"version", "title", "banner", "env", "flags", "steps"}, "workflow")
    if type(config.get("version")) is not int or config["version"] != 1:
        raise UpdateError("workflow: version must be 1")
    string(config.get("title", "Workflow"), "title")
    presentation(config, "workflow")
    environment(config.get("env", {}), "env")
    flags = config.get("flags", {})
    if not isinstance(flags, dict):
        raise UpdateError("flags: expected an object")
    for name, flag in flags.items():
        if not re.fullmatch(r"[a-z][a-z0-9-]*", name) or name in {
            "help",
            "config",
            "dry-run",
            "check",
        }:
            raise UpdateError(f"invalid or reserved flag: {name}")
        fields(flag, {"env", "help"}, f"flag {name}")
        environment({string(flag.get("env"), f"flag {name}.env"): "1"}, f"flag {name}")
        string(flag.get("help"), f"flag {name}.help")
    steps = config.get("steps")
    if not isinstance(steps, list) or not steps:
        raise UpdateError("steps: expected a nonempty list")
    env = dict(os.environ)
    for name, value in config.get("env", {}).items():
        env[name] = expand(value, path.parent, env)
    ids = set()

    def step(item, parallel=False):
        fields(
            item,
            {
                "id",
                "label",
                "verb",
                "command",
                "preview",
                "cwd",
                "env",
                "timeout",
                "output",
                "skip_codes",
                "stop_codes",
                "section",
                "banner",
                "summary",
            },
            "step",
        )
        presentation(item, "step")
        if parallel and ("section" in item or "banner" in item):
            raise UpdateError("put headings on the parallel group, not its members")
        if type(item.get("summary", True)) is not bool:
            raise UpdateError("step.summary: expected a boolean")
        identity = string(item.get("id"), "step.id")
        if identity in ids:
            raise UpdateError(f"duplicate step id: {identity}")
        ids.add(identity)
        string(item.get("label", identity), f"{identity}.label")
        verb = string(item.get("verb", "RUN"), f"{identity}.verb")
        if not re.fullmatch("[A-Z]{1,7}", verb):
            raise UpdateError(f"{identity}: verb must be 1-7 uppercase letters")
        environment(item.get("env", {}), f"{identity}.env")
        step_env = dict(env)
        for key, value in item.get("env", {}).items():
            step_env[key] = expand(value, path.parent, step_env)
        item["_env"] = step_env
        for key in ("command", "preview"):
            if key == "preview" and key not in item:
                continue
            argv = item.get(key)
            if not isinstance(argv, list) or not argv:
                raise UpdateError(f"{identity}.{key}: expected a nonempty argv array")
            item[key] = [
                expand(string(arg, f"{identity}.{key}"), path.parent, step_env)
                for arg in argv
            ]
        cwd = expand(
            string(item.get("cwd", "{config_dir}"), f"{identity}.cwd"),
            path.parent,
            step_env,
        )
        item["cwd"] = str((path.parent / cwd).resolve())
        timeout = item.get("timeout", 300)
        if (
            type(timeout) not in (int, float)
            or not math.isfinite(timeout)
            or timeout <= 0
        ):
            raise UpdateError(f"{identity}: timeout must be positive and finite")
        output = item.get("output", "quiet")
        if output not in {"quiet", "stream"} or parallel and output != "quiet":
            raise UpdateError(
                f"{identity}: parallel steps require quiet output; output must be quiet or stream"
            )
        codes = []
        for key in ("skip_codes", "stop_codes"):
            values = item.get(key, [])
            if not isinstance(values, list) or any(
                type(n) is not int or not 1 <= n <= 255 for n in values
            ):
                raise UpdateError(
                    f"{identity}.{key}: expected exit codes between 1 and 255"
                )
            codes.extend(values)
        if len(set(codes)) != len(codes):
            raise UpdateError(f"{identity}: exit code mappings must not overlap")

    for item in steps:
        if isinstance(item, dict) and "parallel" in item:
            fields(item, {"parallel", "section", "banner"}, "parallel group")
            presentation(item, "parallel group")
            children = item["parallel"]
            if not isinstance(children, list) or not children:
                raise UpdateError("parallel: expected a nonempty list")
            for child in children:
                step(child, parallel=True)
        else:
            step(item)
    return config
