"""Real pointer acceptance with synthetic data; run under dev-check.

Writes GTK snapshots and compositor captures to TASKTJIK_PARITY_DIR or a fresh
temporary directory. The containing display is placed silently on workspace 7.
"""

import sys, os, subprocess, time, json, tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "packages/tjik-gtk/scripts"))
from display import rendering_display

root = Path(
    os.environ.get("TASKTJIK_PARITY_DIR") or tempfile.mkdtemp(prefix="tasktjik-parity-")
)
root.mkdir(parents=True, exist_ok=True)
(root / "fixture.json").write_bytes(
    (Path(__file__).parent / "fixtures/parity.json").read_bytes()
)
native = REPO / "packages/tjik-gtk/target/native/lib"
subprocess.run(
    [
        "cargo",
        "build",
        "--manifest-path",
        str(REPO / "packages/tasktjik-gtk/Cargo.toml"),
        "--example",
        "parity",
        "--locked",
    ],
    env={**os.environ, "PKG_CONFIG_PATH": str(native / "pkgconfig")},
    check=True,
)
(root / "command.json").unlink(missing_ok=True)
protocol = REPO / "apps/runtjik/tests/virtual_pointer.xml"
subprocess.run(
    ["wayland-scanner", "client-header", str(protocol), str(root / "pointer.h")],
    check=True,
)
subprocess.run(
    ["wayland-scanner", "private-code", str(protocol), str(root / "protocol.c")],
    check=True,
)
flags = subprocess.check_output(
    ["pkg-config", "--cflags", "--libs", "wayland-client"], text=True
).split()
subprocess.run(
    [
        "cc",
        str(REPO / "apps/runtjik/tests/pointer.c"),
        str(root / "protocol.c"),
        "-I" + str(root),
        "-o",
        str(root / "pointer"),
        *flags,
    ],
    check=True,
)
with rendering_display():

    def hypr(*args):
        return subprocess.check_output(
            ["hyprctl", "--instance", os.environ["HYPRLAND_INSTANCE_SIGNATURE"], *args],
            text=True,
        )

    hypr(
        "eval",
        'hl.window_rule({name="task-parity",float=true,no_anim=true,match={initial_title="^Tasktjik parity fixture$"}})',
    )
    env = {
        **os.environ,
        "GTK_A11Y": "none",
        "GSK_RENDERER": "cairo",
        "LD_LIBRARY_PATH": str(native),
    }
    with (root / "gtk.log").open("w") as log:
        p = subprocess.Popen(
            [
                str(REPO / "packages/tasktjik-gtk/target/debug/examples/parity"),
                str(root),
            ],
            env=env,
            stdout=log,
            stderr=log,
        )
        pointer = subprocess.Popen(
            [str(root / "pointer"), "GTK"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )

        def event(cmd):
            pointer.stdin.write(cmd + "\n")
            pointer.stdin.flush()
            assert pointer.stdout.readline().strip() == "ok"

        def move(x, y):
            event(f"m {int(x * 10)} {int(y * 10000 / 680)}")

        def inspect():
            return json.loads((root / "inspect.json").read_text())

        def item(name):
            return next(g for g in inspect()["geometry"] if g["name"] == name)

        def command(action, **kw):
            (root / "command.json").write_text(
                json.dumps(dict(sequence=time.time_ns(), action=action, **kw))
            )
            time.sleep(0.4)

        def shot(name):
            command("capture", file=name + ".png")
            subprocess.run(["grim", str(root / (name + "-display.png"))], check=True)

        def drag(source, target, release=True):
            a = item("task-" + source)
            b = item("task-" + target)
            x = a["x"] + 100
            y = a["y"] + 16
            move(x, y)
            event("b 1")
            time.sleep(0.1)
            for i in range(1, 16):
                move(x + (b["x"] + 150 - x) * i / 15, y + (b["y"] + 8 - y) * i / 15)
                time.sleep(0.03)
            time.sleep(0.2)
            if release:
                event("b 0")
                time.sleep(0.5)

        try:
            time.sleep(2)
            hypr("dismissnotify")
            move(990, 670)
            fixture = json.loads((root / "fixture.json").read_text())
            statuses = [
                dict(
                    id="ready",
                    title="Ready task",
                    completions={"previous-period": "2026-11-11T12:00:00Z"},
                    kind="todo",
                    group="All",
                    status="ready",
                    done=False,
                ),
                dict(
                    id="done-old",
                    title="Older completion",
                    kind="todo",
                    group="All",
                    status="done",
                    done=True,
                    completedAt="2026-11-08T12:00:00Z",
                ),
                dict(
                    id="done-new",
                    title="Latest completion",
                    kind="todo",
                    group="All",
                    status="done",
                    done=True,
                    completedAt="2026-11-10T12:00:00Z",
                ),
                dict(
                    id="cancelled",
                    title="Cancelled task",
                    kind="todo",
                    group="All",
                    status="cancelled",
                    done=False,
                ),
                dict(
                    id="blocked",
                    title="Blocked task",
                    kind="todo",
                    group="All",
                    status="blocked",
                    done=False,
                ),
            ]
            command(
                "update",
                state={"tasks": statuses, "groups": ["All"], "today": "2026-11-10"},
            )

            def visible_ids():
                return {
                    g["name"].removeprefix("task-")
                    for g in inspect()["geometry"]
                    if "task-card" in g["classes"]
                }

            def click_filter(name):
                control = item("filter-" + name)
                move(
                    control["x"] + control["width"] / 2,
                    control["y"] + control["height"] / 2,
                )
                event("b 1")
                event("b 0")
                time.sleep(0.3)

            assert visible_ids() == {"ready"}
            for name, matches in [
                ("completed", {"done-old", "done-new"}),
                ("cancelled", {"cancelled"}),
                ("blocked", {"blocked"}),
            ]:
                click_filter(name)
                assert visible_ids() == {"ready"} | matches, (name, visible_ids())
                assert "filter-included" in item("filter-" + name)["classes"]
                click_filter(name)
                assert visible_ids() == matches, (name, visible_ids())
                assert "filter-only" in item("filter-" + name)["classes"]
                shot("gtk-only-" + name)
                click_filter(name)
                assert visible_ids() == {"ready"}
            command("sort", index=6)
            assert visible_ids() == {"ready", "done-old", "done-new"}
            visible = sorted(
                (g for g in inspect()["geometry"] if "task-card" in g["classes"]),
                key=lambda g: g["y"],
            )
            assert [g["name"] for g in visible] == [
                "task-done-new",
                "task-done-old",
                "task-ready",
            ]
            assert "task-completed" in item("task-done-new")["classes"]
            move(990, 670)
            shot("gtk-recent-completions")
            click_filter("completed")
            click_filter("cancelled")
            click_filter("cancelled")
            assert visible_ids() == {"done-old", "done-new", "cancelled"}, (
                "Only categories should combine"
            )
            click_filter("completed")
            click_filter("cancelled")
            command("sort", index=0)
            command("update", state=fixture)
            move(990, 670)
            shot("gtk")
            new = next(g for g in inspect()["geometry"] if g["text"] == "+ New")
            move(new["x"] + new["width"] / 2, new["y"] + new["height"] / 2)
            time.sleep(0.2)
            shot("gtk-primary-hover")

            def layout():
                return [
                    (
                        g["name"],
                        g["text"],
                        *(g[k] for k in ("x", "y", "width", "height")),
                    )
                    for g in inspect()["geometry"]
                    if "task-card" in g["classes"]
                    or "task-title" in g["classes"]
                    or "domain-heading" in g["classes"]
                ]

            assert not any(
                g["tooltip"] == "Task actions" for g in inspect()["geometry"]
            ), "Invented task action menu remains"
            marker_before = item("complete-Work-1")
            before_hover = layout()
            row = item("task-Work-1")
            move(row["x"] + 100, row["y"] + 16)
            time.sleep(0.2)
            marker_after = item("complete-Work-1")
            assert tuple(
                marker_before[k] for k in ("x", "y", "width", "height")
            ) == tuple(marker_after[k] for k in ("x", "y", "width", "height")), (
                "Checkbox changed the marker box"
            )
            shot("gtk-task-hover")
            assert layout() == before_hover, "Hover changed task or domain geometry"
            for x in (row["x"] + 14, row["x"] + row["width"] - 15, 990):
                move(x, row["y"] + 16 if x != 990 else 670)
                time.sleep(0.25)
                assert layout() == before_hover, (
                    "Marker/action hover or leave changed geometry"
                )

            def instances():
                return {
                    g["name"]: g["instance"]
                    for g in inspect()["geometry"]
                    if "task-card" in g["classes"]
                }

            before_selection = instances()
            row = item("task-Work-1")
            move(row["x"] + 100, row["y"] + 16)
            event("b 1")
            event("b 0")
            for _ in range(12):
                time.sleep(0.05)
                assert instances() == before_selection, (
                    "Selecting a task rebuilt the board"
                )
                assert layout() == before_hover, "Selecting a task shifted the board"
            assert item("task-Work-1")["focus"], "Clicked task lost focus"
            for key in ("j", "k", "Return"):
                subprocess.run(["wtype", "-k", key], check=True)
                time.sleep(0.4)
                assert instances() == before_selection, (
                    "Keyboard selection rebuilt the board"
                )
                assert layout() == before_hover, "Keyboard selection shifted the board"
            # Walk the physical column-major layout, crossing stacked domains
            # before moving to the next column, in both directions and aliases.
            rows = sorted(
                (g for g in inspect()["geometry"] if "task-card" in g["classes"]),
                key=lambda g: (g["x"], g["y"]),
            )
            first = rows[0]
            move(first["x"] + 100, first["y"] + 16)
            event("b 1")
            event("b 0")
            time.sleep(0.4)
            index = 0
            for key, direction in (
                ("Down", 1),
                ("Up", -1),
                ("j", 1),
                ("k", -1),
                ("Tab", 1),
                ("ISO_Left_Tab", -1),
            ):
                for _ in rows:
                    index = (index + direction) % len(rows)
                    subprocess.run(["wtype", "-k", key], check=True)
                    time.sleep(0.12)
                    focused = next(
                        g
                        for g in inspect()["geometry"]
                        if "task-card" in g["classes"] and g["focus"]
                    )
                    assert focused["name"] == rows[index]["name"], (
                        key,
                        rows[index]["name"],
                        focused["name"],
                    )
            assert instances() == before_selection, (
                "Vertical navigation rebuilt the board"
            )
            assert layout() == before_hover, "Vertical navigation shifted the board"

            # Horizontal navigation follows geometry across stacked domains.
            for source in ("Work-1", "Home-1", "All-1", "Books-0"):
                for key, direction in (("Left", -1), ("Right", 1)):
                    row = item("task-" + source)
                    move(row["x"] + 100, row["y"] + 16)
                    event("b 1")
                    event("b 0")
                    time.sleep(0.4)
                    rows = [
                        g for g in inspect()["geometry"] if "task-card" in g["classes"]
                    ]
                    candidates = [
                        g for g in rows if (g["x"] - row["x"]) * direction > 1
                    ]
                    expected = row
                    if candidates:
                        distance = min(abs(g["x"] - row["x"]) for g in candidates)
                        adjacent = [
                            g
                            for g in candidates
                            if abs(abs(g["x"] - row["x"]) - distance) < 1
                        ]
                        expected = min(adjacent, key=lambda g: abs(g["y"] - row["y"]))
                    subprocess.run(["wtype", "-k", key], check=True)
                    time.sleep(0.3)
                    focused = next(
                        g
                        for g in inspect()["geometry"]
                        if "task-card" in g["classes"] and g["focus"]
                    )
                    assert focused["name"] == expected["name"], (
                        source,
                        key,
                        expected,
                        focused,
                    )
            shot("gtk-selected")
            # Real subtask disclosure changes only that row, never replaces peers.
            fixture = json.loads((root / "fixture.json").read_text())
            with_steps = json.loads(json.dumps(fixture))
            task = next(t for t in with_steps["tasks"] if t["id"] == "Work-1")
            task["workflowRows"] = [
                {
                    "id": "step-1",
                    "title": "Configure the synthetic organisation and enrolment rules with a deliberately long description that must wrap inside its task column.",
                    "kind": "task",
                    "status": "ready",
                    "depth": 0,
                }
            ]
            task["workflowRows"].extend(
                [
                    {
                        "id": "step-2",
                        "title": "Split capabilities into reusable groups and keep the long descriptive text readable at narrow column widths.",
                        "kind": "task",
                        "status": "done",
                        "depth": 1,
                    },
                    {
                        "id": "step-3",
                        "title": "Validate the new workflow after deployment",
                        "kind": "exit",
                        "status": "ready",
                        "depth": 0,
                    },
                ]
            )
            command("update", state=with_steps)
            row = item("task-Work-1")
            move(row["x"] + 100, row["y"] + 16)
            original_instances = instances()
            original_height = row["height"]
            event("b 1")
            event("b 0")
            time.sleep(0.6)
            assert instances() == original_instances, (
                "Subtask expansion replaced task rows"
            )
            assert item("task-Work-1")["height"] > original_height, (
                "Subtasks did not expand"
            )
            step = item("subtask-step-1")
            assert step["height"] > 32, "Long subtask text did not wrap"
            assert (
                step["x"] >= row["x"]
                and step["x"] + step["width"] <= row["x"] + row["width"]
            ), "Subtask exceeds its column"
            shot("gtk-subtasks")
            subprocess.run(["wtype", "-k", "Down"], check=True)
            time.sleep(0.2)
            assert item("subtask-step-1")["focus"], (
                "Down skipped the first expanded subtask"
            )
            subprocess.run(["wtype", "-k", "Down"], check=True)
            time.sleep(0.2)
            assert item("subtask-step-2")["focus"], "Down skipped the nested subtask"
            subprocess.run(["wtype", "-k", "Up"], check=True)
            time.sleep(0.2)
            assert item("subtask-step-1")["focus"], "Up skipped the previous subtask"
            requests_before = len(inspect()["requests"])
            move(step["x"] + 100, step["y"] + 15)
            event("b 1")
            event("b 0")
            time.sleep(0.4)
            assert len(inspect()["requests"]) == requests_before, (
                "Selecting subtask completed it"
            )
            assert item("subtask-step-1")["focus"], "Subtask selection lost focus"
            complete = item("subtask-complete-step-1")
            move(
                complete["x"] + complete["width"] / 2,
                complete["y"] + complete["height"] / 2,
            )
            time.sleep(0.2)
            shot("gtk-subtask-check")
            event("b 1")
            event("b 0")
            time.sleep(0.3)
            assert inspect()["requests"][-1]["action"] == "subtask-toggle"
            assert inspect()["requests"][-1]["subtask_id"] == "step-1"
            command("error", message="Synthetic completion rejected")
            # Select the parent without clicking it (which would collapse).
            command("open", collection="todo", view="all")
            # Click the progress pips to collapse without affecting completion.
            toggle = next(
                g
                for g in inspect()["geometry"]
                if g["tooltip"] == "Show or hide subtasks"
            )
            move(toggle["x"] + 10, toggle["y"] + toggle["height"] / 2)
            event("b 1")
            event("b 0")
            time.sleep(0.4)
            assert instances() == original_instances, (
                "Subtask collapse replaced task rows"
            )
            assert item("task-Work-1")["height"] == original_height, (
                "Subtask collapse did not restore height"
            )
            command("update", state=fixture)

            drag("Work-1", "All-1", False)
            shot("gtk-drag")
            event("b 0")
            time.sleep(0.7)
            req = inspect()["requests"][-1]
            assert (
                req["id"] == "Work-1"
                and req["action"] == "move"
                and req["group"] == "All"
                and req["before_id"] == ""
            ), req
            shot("gtk-dropped")
            command("error", message="Synthetic move rejected")
            shot("gtk-rollback")
            assert item("task-Work-1")["x"] < 500
            n = len(inspect()["requests"])
            drag("Work-1", "All-1", False)
            subprocess.run(["wtype", "-k", "Escape"], check=True)
            time.sleep(0.2)
            event("b 0")
            time.sleep(0.5)
            assert len(inspect()["requests"]) == n, "Escape dispatched a move"
            command(
                "update",
                state={
                    **json.loads((root / "fixture.json").read_text()),
                    "mutation_accepted": True,
                },
            )
            command("sort", index=4)
            drag("Work-1", "All-1")
            req = inspect()["requests"][-1]
            assert req["before_id"] == "All-1", req
            command("error", message="Synthetic manual rejection")
            state = json.loads((root / "fixture.json").read_text())
            project = dict(
                state["tasks"][0],
                id="project-1",
                title="Launch project",
                kind="project",
                group="Home",
            )
            project["onlyInProject"] = True
            state["tasks"].append(project)
            for index in range(2):
                state["tasks"].append(
                    dict(
                        state["tasks"][0],
                        id=f"project-child-{index}",
                        project="project-1",
                        group="Home",
                        done=index == 0,
                        title=f"Synthetic project child {index}",
                    )
                )
            state["mutation_accepted"] = True
            command("update", state=state)
            progress = item("project-progress-project-1")
            assert progress["height"] == 4, progress
            assert progress["tooltip"] == "Project progress: 1 of 2 complete", progress
            assert not any(
                g["text"] == "Tasks" and g["type"] == "GtkLabel" and g["y"] > 100
                for g in inspect()["geometry"]
            ), "Project still has a Tasks button"
            shot("gtk-project-progress")
            drag("Work-1", "project-1")
            req = inspect()["requests"][-1]
            assert req["action"] == "edit" and req["fields"] == {
                "project": "project-1",
                "group": "Home",
            }, req
            command("error", message="Synthetic project rejection")
            # Empty domains become targets only during the gesture.
            drag("Work-1", "All-1", False)
            target = next(
                g
                for g in inspect()["geometry"]
                if "empty-drop-target" in g["classes"] and g["text"] == "Empty"
            )
            move(target["x"] + target["width"] / 2, target["y"] + target["height"] / 2)
            time.sleep(0.2)
            event("b 0")
            time.sleep(0.5)
            req = inspect()["requests"][-1]
            assert req["group"] == "Empty", req
            command("error", message="Synthetic empty-domain rejection")
            # A refresh during a gesture must not silently adopt a newer source revision.
            drag("Work-1", "All-1", False)
            for t in state["tasks"]:
                if t["id"] == "Work-1":
                    t["revision"] = 99
            command("update", state=state)
            event("b 0")
            time.sleep(0.5)
            assert inspect()["requests"][-1]["revision"] == 1, inspect()["requests"][-1]
            command("error", message="Synthetic stale revision rejected")
            command(
                "update",
                state={
                    **json.loads((root / "fixture.json").read_text()),
                    "mutation_accepted": True,
                },
            )
            # The original hover action opens a new subtask draft, not a menu.
            row = item("task-Work-1")
            move(row["x"] + 100, row["y"] + 16)
            time.sleep(0.2)
            add = next(
                g
                for g in inspect()["geometry"]
                if g["tooltip"] == "Add subtask"
                and g["y"] >= row["y"]
                and g["y"] < row["y"] + row["height"]
            )
            before_add = len(inspect()["requests"])
            move(add["x"] + add["width"] / 2, add["y"] + add["height"] / 2)
            event("b 1")
            event("b 0")
            time.sleep(0.3)
            assert inspect()["state"]["gtk_view"] == "editor", (
                "Add subtask did not open the editor"
            )
            assert len(inspect()["requests"]) == before_add, (
                "Add subtask persisted before saving"
            )
            subprocess.run(["wtype", "-k", "Escape"], check=True)
            time.sleep(0.3)
            subprocess.run(["wtype", "-k", "n"], check=True)
            time.sleep(0.5)
            shot("gtk-editor")
            print(
                "PASS: spatial column navigation, project progress, stable hover and selection, local subtask disclosure, automatic and manual drops, project assignment, empty domains, refresh isolation, rollback and Escape cancellation"
            )
        finally:
            pointer.terminate()
            pointer.wait()
            p.terminate()
            p.wait()

print(f"Comparison artifacts: {root}")
