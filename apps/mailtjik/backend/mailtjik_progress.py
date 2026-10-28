"""Private reader progress: counts and timings only, never mail metadata."""
import hashlib
import threading
import time


class QueryProgress:
    def __init__(self, store, parts, emit=None, clock=time.monotonic):
        self.store, self.parts, self.emit, self.clock = store, parts, emit, clock
        self.lock = threading.Lock()
        self.stopping = threading.Event()
        self.thread = None
        self.started = clock()
        self.ready = self.searched = 0
        self.phase = 'indexes'
        self.cached = 0
        self.transfers = []
        for part in parts:
            index = part['index']
            target = store.workdir / 'indexes' / (index['plaintext_sha256'] + '.sqlite')
            if target.is_file():
                self.cached += 1
            else:
                path = store.cache / (hashlib.sha256(index['path'].encode()).hexdigest() + '.age')
                self.transfers.append((path, int(index['cipher_bytes'])))
        self.total_bytes = sum(size for _, size in self.transfers)
        self.received = self.initial_bytes = self.download_bytes()

    def download_bytes(self):
        total = 0
        for path, size in self.transfers:
            sizes = [0]
            for candidate in (path, path.with_suffix('.partial')):
                try:
                    sizes.append(candidate.stat().st_size)
                except FileNotFoundError:
                    pass
            total += min(size, max(sizes))
        return total

    def snapshot(self):
        with self.lock:
            elapsed = max(0., self.clock() - self.started)
            self.received = max(self.received, self.download_bytes())
            phase = self.phase
            eta = None
            if phase == 'indexes':
                if self.received < self.total_bytes:
                    phase = 'downloading'
                    transferred = self.received - self.initial_bytes
                    if elapsed >= 2 and transferred >= 64 * 1024:
                        eta = (self.total_bytes - self.received) * elapsed / transferred
                else:
                    phase = 'verifying' if self.transfers else 'checking'
            return {
                'phase': phase, 'total_indexes': len(self.parts),
                'ready_indexes': self.ready, 'cached_indexes': self.cached,
                'searched_indexes': self.searched, 'downloaded_bytes': self.received,
                'total_download_bytes': self.total_bytes,
                'elapsed_seconds': elapsed, 'eta_seconds': eta,
            }

    def send(self):
        if self.emit:
            self.emit(self.snapshot())

    def monitor(self):
        while not self.stopping.wait(.25):
            self.send()

    def __enter__(self):
        if self.emit:
            self.send()
            self.thread = threading.Thread(target=self.monitor, daemon=True)
            self.thread.start()
        return self

    def index_ready(self):
        with self.lock:
            self.ready += 1

    def searching(self):
        with self.lock:
            self.phase = 'searching'

    def index_searched(self):
        with self.lock:
            self.searched += 1

    def __exit__(self, *_):
        self.stopping.set()
        if self.thread:
            self.thread.join()
