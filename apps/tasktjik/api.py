"""Client enrollment commands and dispatch to the Rust authority binary."""
import argparse
import json
import os
from pathlib import Path


def command(args):
    binary = Path(__file__).parent / 'tasktjik-api'
    if not binary.is_file():
        binary = Path(__file__).parent / 'target/release/tasktjik-api'
    if args[0] == 'api':
        os.execv(str(binary), [str(binary), *args[1:]])
    parser = argparse.ArgumentParser(prog='tasktjik')
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('connect')
    p.add_argument('url')
    p.add_argument('--notifications', action='store_true')
    sub.add_parser('devices')
    for name in ('import-json', 'snapshot'):
        p = sub.add_parser(name)
        p.add_argument('--directory', required=True)
        p.add_argument('path')
    options = parser.parse_args(args)
    if options.command == 'connect':
        from client import connect
        print(json.dumps(connect(options.url, options.notifications)))
    elif options.command == 'devices':
        from client import http_request
        from tasktjik import config
        print(json.dumps(http_request(config()['plugin_settings']['Todo']['url'], 'devices')))
    else:
        database = Path(options.directory).expanduser() / 'tasks.sqlite3'
        os.execv(str(binary), [str(binary), options.command, str(database), str(Path(options.path).expanduser())])
