"""One live terminal row per concurrent task, with plain output for logs."""

from contextlib import contextmanager
import os
import sys
import threading
import time

from . import runtime


def enabled():
    return os.environ.get("TJIKUP_PROGRESS", "1" if sys.stdout.isatty() else "0") == "1" and os.environ.get("TERM") != "dumb"


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
                    with self._frame():
                        self._clear()
                        self._draw()

    def _clear(self):
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
            for verb, label, started in self.rows.values():
                self._print(verb, spinner, label, f"{now - started:.1f}s")
            self.height = len(self.rows)

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
