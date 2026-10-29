"""Bundled, asynchronous word lookups. No credentials or disk query history."""
from concurrent.futures import ThreadPoolExecutor
from html import unescape
import json
import re
import subprocess
import time
from urllib.error import HTTPError
from urllib.parse import quote, urlencode, urlparse
from urllib.request import Request, urlopen

API_VERSION = 1
DEFAULT_OPTIONS = {
    "define_sources": ["free_dictionary", "wiktionary"],
    "synonym_sources": ["datamuse", "free_dictionary"],
    "max_results_per_source": 12,
    "timeout_seconds": 5,
}
SOURCES = {
    "define_sources": {"free_dictionary", "wiktionary", "urban_dictionary"},
    "synonym_sources": {"datamuse", "free_dictionary"},
}


def validate_options(options):
    if set(options) - set(DEFAULT_OPTIONS):
        raise ValueError("Unknown Words setting")
    for key, allowed in SOURCES.items():
        values = options[key]
        if not isinstance(values, list) or any(not isinstance(v, str) or v not in allowed for v in values):
            raise ValueError(f"Words.{key}: choose from {', '.join(sorted(allowed))}")
        if len(values) != len(set(values)):
            raise ValueError(f"Words.{key}: duplicate source")
    for key, upper in (("max_results_per_source", 50), ("timeout_seconds", 15)):
        if type(options[key]) is not int or not 1 <= options[key] <= upper:
            raise ValueError(f"Words.{key} must be an integer between 1 and {upper}")


def plain(value):
    return unescape(re.sub(r"<[^>]+>", "", str(value))).strip()


def fetch(url, timeout):
    request = Request(url, headers={"User-Agent": "Runtjik/0.8 (dictionary lookup)", "Accept": "application/json"})
    try:
        with urlopen(request, timeout=timeout) as response:
            raw = response.read(2_000_001)
            if len(raw) > 2_000_000:
                raise ValueError("Response exceeds 2 MB")
            return json.loads(raw)
    except HTTPError as exc:
        if exc.code == 404:
            return []
        raise


def lookup(source, command, term, timeout, limit):
    """Return (definition or synonym, usage/context, source URL) tuples."""
    encoded = quote(term, safe="")
    rows = []
    if source == "free_dictionary":
        data = fetch("https://api.dictionaryapi.dev/api/v2/entries/en/" + encoded, timeout)
        for entry in data:
            url = "https://en.wiktionary.org/wiki/" + encoded
            for meaning in entry.get("meanings", []):
                part = meaning.get("partOfSpeech", "")
                if command == "/synonym":
                    synonyms = list(meaning.get("synonyms", []))
                    for definition in meaning.get("definitions", []):
                        synonyms.extend(definition.get("synonyms", []))
                    rows.extend((word, part, url) for word in dict.fromkeys(synonyms))
                else:
                    rows.extend((d["definition"], part + (" · Example: " + d["example"] if d.get("example") else ""), url)
                                for d in meaning.get("definitions", []) if d.get("definition"))
    elif source == "wiktionary":
        data = fetch("https://en.wiktionary.org/api/rest_v1/page/definition/" + encoded, timeout)
        for meaning in data.get("en", []) if isinstance(data, dict) else []:
            for d in meaning.get("definitions", []):
                examples = [plain(v) for v in d.get("examples", [])]
                rows.append((plain(d.get("definition", "")), meaning.get("partOfSpeech", "") +
                             (" · Examples: " + " / ".join(examples) if examples else ""),
                             "https://en.wiktionary.org/wiki/" + encoded))
    elif source == "urban_dictionary":
        data = fetch("https://api.urbandictionary.com/v0/define?" + urlencode({"term": term}), timeout)
        for d in data.get("list", []) if isinstance(data, dict) else []:
            clean = lambda text: str(text).replace("[", "").replace("]", "")
            rows.append((clean(d.get("definition", "")), "Example: " + clean(d.get("example", "")),
                         "https://www.urbandictionary.com/define.php?" + urlencode({"term": term, "defid": d.get("defid", "")})))
    elif source == "datamuse":
        data = fetch("https://api.datamuse.com/words?" + urlencode({"rel_syn": term, "max": limit}), timeout)
        rows.extend((d["word"], "Synonym of " + term, "https://en.wiktionary.org/wiki/" + quote(d["word"], safe="")) for d in data)
    return rows[:limit]


class Provider:
    name = "Words"
    commands = {"/define": "Definitions and usage examples · Live results",
                "/synonym": "Thesaurus synonyms · Live results"}

    def __init__(self, demo=False):
        self.demo = demo
        self.completed = None
        self.rows = []
        self.pending = False
        self.executor = ThreadPoolExecutor(max_workers=3)
        self.futures = {}
        self.key = None

    def row(self, key, name, subtitle, **data):
        return {"id": "Words:" + key, "provider": self.name, "name": name,
                "subtitle": subtitle, "icon": "accessories-dictionary", "data": {"keep_open": True, **data}}

    def query(self, query, settings):
        command, _, term = query.strip().partition(" ")
        if command not in self.commands:
            return []
        term = term.strip()
        if not term:
            self.pending = False
            return [self.row("hint", "Type a word to look up", "Definitions and synonyms appear here as you type", disabled=True)]
        options = settings["plugin_settings"][self.name]
        sources = options["define_sources" if command == "/define" else "synonym_sources"]
        key = (command, term, tuple(sources), options['max_results_per_source'], options['timeout_seconds'])
        if key != self.key:
            for future in self.futures.values():
                future.cancel()
            self.key = key
            self.completed = query
            self.rows = []
            self.futures = {} if self.demo else {
                source: self.executor.submit(lookup, source, command, term, options['timeout_seconds'], options['max_results_per_source'])
                for source in sources}
        if self.demo:
            self.pending = False
            return [self.row("demo1", "To explain the meaning of a word or expression." if command == '/define' else 'explain',
                             "Synthetic dictionary · Verb · Example: Please define this term."),
                    self.row("demo2", "To describe the limits or essential qualities of something." if command == '/define' else 'describe',
                             "Synthetic dictionary · Verb · Example: These principles define our work.")]
        rows = []
        self.pending = any(not future.done() for future in self.futures.values())
        for source, future in self.futures.items():
            if not future.done():
                continue
            try:
                values = future.result()
                for i, (name, example, url) in enumerate(values):
                    rows.append(self.row(f"{source}:{i}", name, source.replace('_', ' ') + " · " + example, url=url))
                if not values:
                    rows.append(self.row(source + ':empty', 'No entries found', source.replace('_', ' '), disabled=True))
            except Exception as exc:
                rows.append(self.row(source + ':error', 'Source unavailable', f'{source}: {exc}', disabled=True))
        if self.pending:
            rows.append(self.row('loading', 'Looking up “' + term + '”…', 'Results appear as each source responds', disabled=True))
        elif not rows:
            rows.append(self.row('empty', 'No sources enabled', 'Edit /config plugin_settings.Words', disabled=True))
        self.rows = rows
        return rows

    def activate(self, entry, settings, mode):
        if entry.data.get('url'):
            url = entry.data['url']
            parsed = urlparse(url)
            if parsed.scheme != 'https' or parsed.hostname not in ('en.wiktionary.org', 'www.urbandictionary.com'):
                raise ValueError('Invalid word source URL')
            if not self.demo:
                subprocess.Popen(['xdg-open', url], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
            return {'kind': 'message', 'message': 'Opened source · results stay here'}
        return {'kind': 'message', 'message': 'Results update as you type'}
