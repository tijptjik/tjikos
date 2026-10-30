#!/usr/bin/env python3
"""Firefox native messaging host; one private socket per browser connection."""
import json
import os
import queue
import socket
import struct
import sys
import threading
import time
import uuid

from browser import runtime_directory

MAX_MESSAGE = 4 * 1024 * 1024


def read_exact(stream, size):
    data = bytearray()
    while len(data) < size:
        part = stream.read(size - len(data))
        if not part:
            raise EOFError()
        data.extend(part)
    return bytes(data)


def read_message(stream):
    size, = struct.unpack('=I', read_exact(stream, 4))
    if size > MAX_MESSAGE:
        raise ValueError('Native message too large')
    return json.loads(read_exact(stream, size))


def write_message(stream, message):
    data = json.dumps(message).encode()
    if len(data) > MAX_MESSAGE:
        raise ValueError('Native message too large')
    stream.write(struct.pack('=I', len(data)) + data)
    stream.flush()


def marked_window(marker):
    from runtjik import windows
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        matches = [client for client in windows() if client.get('title', '').startswith(marker)]
        if len(matches) == 1:
            return matches[0]
        time.sleep(0.05)
    raise RuntimeError('Could not identify the browser window in Hyprland')


def focus_marker(marker):
    from runtjik import focus_window
    focus_window(marked_window(marker))


def browser_action(message, exchange):
    close_window = message['op'] == 'close' and message.get('tab') is None
    if message['op'] != 'focus' and not close_window:
        return exchange(message)
    marker = '[runtjik-' + uuid.uuid4().hex + '] '
    try:
        response = exchange({**message, 'op': 'focus', 'marker': marker})
        if close_window:
            from runtjik import close_windows
            close_windows([marked_window(marker)])
            return {'message': 'Hyprland close requested; Zen may ask for confirmation'}
        focus_marker(marker)
        return response
    finally:
        try:
            exchange({'op': 'release', 'window': message.get('window')})
        except Exception:
            # Closing the last window can also terminate the extension connection.
            # The extension has its own fallback restoration timer.
            pass


def serve():
    os.umask(0o077)
    directory = runtime_directory()
    directory.mkdir(mode=0o700, exist_ok=True)
    if directory.is_symlink() or directory.stat().st_uid != os.getuid() or directory.stat().st_mode & 0o077:
        raise RuntimeError('Browser runtime directory must be private')
    path = directory / (uuid.uuid4().hex + '.sock')
    inbox = queue.Queue(maxsize=16)
    stopped = threading.Event()

    def reader():
        try:
            while True:
                inbox.put(read_message(sys.stdin.buffer), timeout=1)
        except (EOFError, ValueError, OSError, queue.Full):
            stopped.set()

    threading.Thread(target=reader, daemon=True).start()

    def exchange(message):
        ident = uuid.uuid4().hex
        write_message(sys.stdout.buffer, {**message, 'id': ident})
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            try:
                response = inbox.get(timeout=0.1)
            except queue.Empty:
                if stopped.is_set():
                    raise RuntimeError('Browser disconnected')
                continue
            if response.get('id') == ident:
                if 'error' in response:
                    raise RuntimeError(response['error'])
                return response
        raise RuntimeError('Browser response timed out')

    try:
        with socket.socket(socket.AF_UNIX) as server:
            server.bind(str(path))
            server.listen(4)
            server.settimeout(0.25)
            while not stopped.is_set():
                try:
                    client, _ = server.accept()
                except socket.timeout:
                    continue
                with client:
                    client.settimeout(12)
                    try:
                        with client.makefile('rb') as stream:
                            raw = stream.readline(65537)
                        if len(raw) > 65536:
                            raise ValueError('Request too large')
                        message = json.loads(raw)
                        if message.get('op') not in ('list', 'focus', 'details', 'close'):
                            raise ValueError('Unknown browser operation')
                        response = browser_action(message, exchange)
                    except Exception as exc:
                        response = {'error': str(exc)}
                    try:
                        client.sendall(json.dumps(response).encode() + b'\n')
                    except OSError:
                        pass
    finally:
        path.unlink(missing_ok=True)


if __name__ == '__main__':
    serve()
