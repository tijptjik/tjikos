"""One live terminal row per concurrent task, with plain output for logs."""

from contextlib import contextmanager
import os
import re
import sys
import threading
import time

from . import runtime


def enabled():
    return os.environ.get("TJIKUP_PROGRESS", "1" if sys.stdout.isatty() else "0") == "1" and os.environ.get("TERM") != "dumb"


SGR = re.compile(r"\x1b\[[0-9;]*m")


def status_cells(value):
    """Styled display cells, including space occupied by wide characters."""
    cells, style, offset = [], "", 0

    def append(text):
        for char in text:
            width = runtime.display_width(char)
            if not width and cells:
                index = len(cells) - 1
                while index and not cells[index][0]:
                    index -= 1
                previous, color = cells[index]
                cells[index] = (previous + char, color)
            else:
                cells.append((char, style))
                cells.extend(("", style) for _ in range(max(0, width - 1)))

    for match in SGR.finditer(value):
        append(value[offset:match.start()])
        style = "" if match.group() == "\033[0m" else style + match.group()
        offset = match.end()
    append(value[offset:])
    return cells


def changed_spans(before, after, *, group=1):
    """Return changed cell runs; grouped cells support Kitty's 2x2 text blocks."""
    before, after = status_cells(before), status_cells(after)
    width = ((max(len(before), len(after)) + group - 1) // group) * group
    before.extend([(" ", "")] * (width - len(before)))
    after.extend([(" ", "")] * (width - len(after)))
    dirty = set()
    for column in range(width):
        if before[column] != after[column]:
            start = column // group * group
            end = start + group
            # Group boundaries must not split a wide character in either row.
            while start and (not before[start][0] or not after[start][0]):
                start = max(0, start - group)
            while end < width and (not before[end][0] or not after[end][0]):
                end = min(width, end + group)
            dirty.update(range(start, end))
    start = None
    for column in range(width + 1):
        if column in dirty:
            if start is None:
                start = column
        elif start is not None:
            yield start, after[start:column]
            start = None


def normal_text(cells):
    return "".join("\033[0m" + style + char for char, style in cells if char) + "\033[0m"


class Progress:
    def __init__(self, *, stream=None, live=None):
        self.stream = stream or sys.stdout
        self.live = enabled() if live is None else live
        self.rows = {}
        self.height = 0
        self.lock = threading.RLock()
        self.stop = threading.Event()
        self.thread = None
        self.cursor_hidden = False
        self.rendered = {}
        self.columns = None

    def __enter__(self):
        if self.live:
            self.thread = threading.Thread(target=self._tick, daemon=True)
            self.thread.start()
        return self

    def __exit__(self, *_):
        self.stop.set()
        if self.thread:
            self.thread.join()
        with self.lock, self._frame(active=False):
            self._clear()
            for verb, label, started in self.rows.values():
                self._print(verb, "!", label, "interrupted")
            self.rows.clear()

    def _cursor(self, hidden):
        if self.live and hidden != self.cursor_hidden:
            self.stream.write("\033[?25l" if hidden else "\033[?25h")
            self.cursor_hidden = hidden

    @contextmanager
    def _frame(self, *, active=True):
        """Present erasure, text and image placeholders as one terminal update."""
        if self.live:
            self.stream.write("\033[?2026h")
            self._cursor(True)
        try:
            yield
        except BaseException:
            active = False
            raise
        finally:
            self._cursor(bool(active and self.rows))
            if self.live:
                self.stream.write("\033[?2026l")
            self.stream.flush()

    def _tick(self):
        while not self.stop.wait(0.12):
            with self.lock:
                if self.rows:
                    self._refresh()

    def _patch(self, before, after):
        return "".join(
            f"\033[{column + 1}G" + normal_text(cells)
            for column, cells in changed_spans(runtime.format_status(*before),
                                               runtime.format_status(*after))
        )

    def _refresh(self):
        # Subclasses with static panels or temporarily suspended output have
        # no cached status rows to animate.
        if not self.height or not self.rendered:
            return
        if runtime.terminal_columns() != self.columns:
            with self._frame():
                self._clear()
                self._draw()
            return
        now = time.monotonic()
        spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"[int(now * 8) % 10]
        patches = []
        row_height = self.height // len(self.rows)
        for index, (key, (verb, label, started)) in enumerate(self.rows.items()):
            current = (verb, spinner, label, f"{now - started:.1f}s")
            previous = self.rendered.get(key)
            if previous is not None:
                patch = self._patch(previous, current)
                if patch:
                    patches.append(f"\033[{self.height - index * row_height}A" + patch
                                   + f"\033[{self.height - index * row_height}B")
            self.rendered[key] = current
        if patches:
            with self._frame():
                # Keep the terminal cursor parked below the active rows. No
                # erasure, newlines, labels or image placeholders on a tick.
                self.stream.write("\0337" + "".join(patches) + "\0338")

    def _clear(self):
        self.rendered.clear()
        if self.height:
            self.stream.write(f"\r\033[{self.height}A")
            for _ in range(self.height):
                self.stream.write("\033[2K\n")
            self.stream.write(f"\033[{self.height}A\r")
            self.height = 0

    def _print(self, verb, icon, label, note):
        self.stream.write(runtime.format_status(verb, icon, label, note) + "\n")

    def _draw(self):
        if self.live:
            now = time.monotonic()
            spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"[int(now * 8) % 10]
            self.rendered.clear()
            for key, (verb, label, started) in self.rows.items():
                fields = (verb, spinner, label, f"{now - started:.1f}s")
                self._print(*fields)
                self.rendered[key] = fields
            self.height = len(self.rows)
            self.columns = runtime.terminal_columns()

    def start(self, key, label, verb="UPDATE"):
        with self.lock, self._frame():
            self._clear()
            self.rows[key] = (verb, label, time.monotonic())
            self._draw()

    def finish(self, key, label, *, verb="UPDATE", icon="✓", note="", record=True):
        with self.lock, self._frame():
            self._clear()
            self.rows.pop(key, None)
            if record:
                runtime.record(verb, icon, label, note)
            self._print(verb, icon, label, note)
            self._draw()

    def discard(self, key):
        with self.lock, self._frame():
            self._clear()
            self.rows.pop(key, None)
            self._draw()

    @contextmanager
    def output(self):
        """Serialize headings/diagnostics without overwriting active rows."""
        with self.lock:
            # Never hold synchronized output across diagnostics or a prompt.
            with self._frame(active=False):
                self._clear()
            try:
                yield
            finally:
                with self._frame():
                    self._draw()
