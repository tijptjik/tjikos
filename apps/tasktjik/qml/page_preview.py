"""Local page previews: isolated browser renders with site metadata as a fallback."""
import hashlib
from html.parser import HTMLParser
import json
import os
import signal
import shutil
import subprocess
from pathlib import Path
import sys
import tempfile
import time
from urllib.parse import urljoin, urlsplit
from urllib.request import Request, urlopen


class Metadata(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.values = {}
        self.icons = []
        self.in_title = False
        self.title = ''

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'meta':
            key = (attrs.get('property') or attrs.get('name') or '').lower()
            self.values.setdefault(key, attrs.get('content', '').strip())
        if tag == 'link' and set(attrs.get('rel', '').lower().split()) & {'icon', 'apple-touch-icon'}:
            if attrs.get('href'):
                self.icons.append(attrs['href'])
        if tag == 'title':
            self.in_title = True

    def handle_endtag(self, tag):
        if tag == 'title':
            self.in_title = False

    def handle_data(self, data):
        if self.in_title:
            self.title += data


def fetch(url, limit):
    if urlsplit(url).scheme not in ('https', 'http'):
        raise ValueError('Only web pages support previews')
    request = Request(url, headers={'User-Agent': 'Tasktjik/1.0 (saved-page preview)', 'Accept': 'text/html,image/*'})
    with urlopen(request, timeout=5) as response:
        if urlsplit(response.url).scheme not in ('https', 'http'):
            raise ValueError('Unsupported redirect')
        data = response.read(limit + 1)
        if len(data) > limit:
            raise ValueError('Preview too large')
        return data, response.headers, response.url


def atomic_write(path, data):
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
        output.write(data)
        temporary = Path(output.name)
    temporary.replace(path)


def favicon(url, icons, cache, key):
    candidates = list(dict.fromkeys([urljoin(url, icon) for icon in icons] + [urljoin(url, '/favicon.ico')]))
    for candidate in candidates[:3]:
        try:
            data, headers, _ = fetch(candidate, 512 * 1024)
            extension = {'image/png': '.png', 'image/svg+xml': '.svg', 'image/x-icon': '.ico',
                         'image/vnd.microsoft.icon': '.ico', 'image/jpeg': '.jpg',
                         'image/webp': '.webp', 'image/gif': '.gif'}.get(headers.get_content_type())
            if not extension and data.startswith(b'\x00\x00\x01\x00'):
                extension = '.ico'
            if extension:
                target = cache / (key + '-favicon' + extension)
                atomic_write(target, data)
                return target.as_uri()
        except TimeoutError:
            raise
        except (OSError, ValueError):
            continue
    return None


def render_failure(document):
    parser = Metadata()
    parser.feed(document)
    title = parser.title.strip().lower()
    if any(word in title for word in ('403', 'access denied', 'request blocked',
                                     'request could not be satisfied', 'just a moment', 'attention required')):
        return 'blocked'
    if any(word in title for word in ('sign in', 'log in', 'login')):
        return 'sign-in'
    if 'class="neterror"' in document or 'id="main-frame-error"' in document:
        return 'failed'
    return ''


def render_page(url, cache, key):
    browser = next((path for name in ('google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser')
                    if (path := shutil.which(name))), None)
    if not browser:
        return {'renderError': 'browser-unavailable'}
    # Each render gets an empty profile; never read the user's authenticated session.
    with tempfile.TemporaryDirectory(prefix='render-', dir=cache) as temporary:
        directory = Path(temporary)
        screenshot = directory / 'page.png'
        command = [browser, '--headless=new', '--no-first-run', '--no-default-browser-check',
                   '--disable-background-networking', '--disable-extensions', '--disable-sync',
                   '--disable-crash-reporter', '--mute-audio', '--hide-scrollbars',
                   '--window-size=1280,800', '--virtual-time-budget=7000',
                   '--run-all-compositor-stages-before-draw',
                   '--user-data-dir=' + str(directory / 'profile'),
                   '--dump-dom', '--screenshot=' + str(screenshot), url]
        output = (directory / 'document.html').open('w+b')
        process = subprocess.Popen(command, stdout=output, stderr=subprocess.DEVNULL,
                                   start_new_session=True, env={**os.environ, 'TMPDIR': temporary})
        try:
            process.wait(timeout=25)
            if process.returncode != 0 or not screenshot.is_file():
                return {'renderError': 'failed'}
            output.seek(0)
            document = output.read(4 * 1024 * 1024).decode('utf-8', errors='replace')
            failure = render_failure(document)
            if failure:
                return {'renderError': failure}
            data = screenshot.read_bytes()
            if not data.startswith(b'\x89PNG\r\n\x1a\n') or len(data) > 8 * 1024 * 1024:
                return {'renderError': 'failed'}
            target = cache / (key + '-page.png')
            atomic_write(target, data)
            return {'image': target.as_uri(), 'imageKind': 'page'}
        except subprocess.TimeoutExpired:
            return {'renderError': 'timeout'}
        finally:
            # Chromium may leave children after its main process exits or times out.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            output.close()


def preview(url):
    if urlsplit(url).scheme not in ('http', 'https'):
        return {}
    cache = Path(os.environ.get('XDG_CACHE_HOME', str(Path.home() / '.cache'))) / 'tasktjik/page-previews'
    cache.mkdir(parents=True, exist_ok=True, mode=0o700)
    key = hashlib.sha256(url.encode()).hexdigest()
    receipt = cache / (key + '.json')
    if receipt.exists() and time.time() - receipt.stat().st_mtime < 86400:
        try:
            saved = json.loads(receipt.read_text())
            if saved.get('previewVersion') == 4 and (saved.get('imageKind') == 'page' or time.time() - receipt.stat().st_mtime < 300):
                return saved
        except (ValueError, OSError):
            pass
    result = {}
    icons = []
    resolved = url
    signal.alarm(12)
    try:
        data, headers, resolved = fetch(url, 2 * 1024 * 1024)
        if headers.get_content_type() not in ('text/html', 'application/xhtml+xml'):
            raise ValueError('Not a page')
        parser = Metadata()
        parser.feed(data.decode(headers.get_content_charset() or 'utf-8', errors='replace'))
        meta = parser.values
        icons = parser.icons
        result = {'description': (meta.get('og:description') or meta.get('description') or meta.get('twitter:description') or '')[:600],
                  'title': (meta.get('og:title') or parser.title.strip())[:300]}
        image = meta.get('og:image') or meta.get('twitter:image')
        if image:
            try:
                data, headers, _ = fetch(urljoin(resolved, image), 5 * 1024 * 1024)
                extension = {'image/jpeg': '.jpg', 'image/png': '.png', 'image/webp': '.webp', 'image/gif': '.gif'}.get(headers.get_content_type())
                if extension:
                    target = cache / (key + extension)
                    atomic_write(target, data)
                    result['image'] = target.as_uri()
                    result['imageKind'] = 'metadata'
            except Exception:
                pass
    except Exception:
        result['unavailable'] = True
    finally:
        signal.alarm(0)
    # A successful page fetch avoids caching browser error pages for unreachable URLs.
    if not result.get('unavailable'):
        try:
            result.update(render_page(url, cache, key))
        except (OSError, ValueError):
            pass
    if result.get('renderError') in ('blocked', 'sign-in') or not result.get('image'):
        signal.alarm(8)
        try:
            icon = favicon(resolved, icons, cache, key)
            if icon:
                result['image'] = icon
                result['imageKind'] = 'favicon'
        except (OSError, ValueError):
            pass
        finally:
            signal.alarm(0)
    result['previewVersion'] = 4
    atomic_write(receipt, json.dumps(result).encode())
    return result


def deadline(signum, frame):
    raise TimeoutError('Preview deadline exceeded')


if __name__ == '__main__':
    signal.signal(signal.SIGALRM, deadline)
    try:
        print(json.dumps(preview(sys.stdin.read().strip())))
    except Exception:
        print('{}')
