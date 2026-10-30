"""Read-only Hyprland lock observer using hyprland-lock-notify-v1.

Only binds the notification protocol; never acquires or releases a session lock.
The second display sync establishes the initial unlocked state after subscription.
Unsupported/disconnected compositors remain unknown (the UI fails closed).
Protocol: https://github.com/hyprwm/hyprland-protocols/blob/main/protocols/hyprland-lock-notify-v1.xml
"""
import os
import socket
import struct
import time


class LockObserver:
    def __init__(self):
        self.socket = None
        self.state = "unknown"
        self.buffer = b""
        self.bound = False
        self.retry_at = 0

    def close(self):
        if self.socket:
            self.socket.close()
        self.socket = None
        self.state = "unknown"
        self.buffer = b""
        self.bound = False
        self.retry_at = time.monotonic() + 2

    def send(self, object_id, opcode, payload=b""):
        self.socket.sendall(struct.pack("=II", object_id, ((len(payload) + 8) << 16) | opcode) + payload)

    def connect(self):
        if self.socket or time.monotonic() < self.retry_at:
            return
        try:
            display = os.environ.get("WAYLAND_DISPLAY", "wayland-0")
            address = display if display.startswith("/") else os.path.join(os.environ["XDG_RUNTIME_DIR"], display)
            self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.socket.settimeout(0.5)
            self.socket.connect(address)
            self.send(1, 1, struct.pack("=I", 2))  # wl_display.get_registry
            self.send(1, 0, struct.pack("=I", 3))  # initial registry roundtrip
        except (OSError, KeyError):
            self.close()

    def feed(self, data):
        self.buffer += data
        while len(self.buffer) >= 8:
            object_id, header = struct.unpack_from("=II", self.buffer)
            size, opcode = header >> 16, header & 0xffff
            if size < 8 or size % 4:
                raise ValueError("Invalid Wayland event size")
            if len(self.buffer) < size:
                return
            payload = self.buffer[8:size]
            self.buffer = self.buffer[size:]
            if object_id == 1 and opcode == 0:
                raise ValueError("Wayland protocol error")
            if object_id == 2 and opcode == 0:  # registry.global
                name, length = struct.unpack_from("=II", payload)
                interface = payload[8:8 + length].rstrip(b"\0")
                if interface == b"hyprland_lock_notifier_v1" and not self.bound:
                    wire_name = interface + b"\0"
                    padded = wire_name + b"\0" * (-len(wire_name) % 4)
                    self.send(2, 0, struct.pack("=II", name, len(wire_name)) + padded + struct.pack("=II", 1, 4))
                    self.send(4, 1, struct.pack("=I", 5))  # get_lock_notification
                    self.send(1, 0, struct.pack("=I", 6))  # subscription roundtrip
                    self.bound = True
            elif object_id == 5 and opcode in (0, 1):
                self.state = "locked" if opcode == 0 else "unlocked"
            elif object_id == 6 and opcode == 0 and self.state == "unknown":
                self.state = "unlocked"

    def read(self):
        try:
            data = self.socket.recv(65536)
            if not data:
                self.close()
            else:
                self.feed(data)
        except (OSError, ValueError, struct.error):
            self.close()
