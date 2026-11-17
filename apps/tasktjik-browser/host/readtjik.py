#!/usr/bin/env python3
"""Readtjik's restricted Firefox native host; credentials stay with Tasktjik."""
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
from urllib.parse import urlsplit
import uuid

MAX_MESSAGE = 1024 * 1024 - 4096


class Rejected(ValueError):
    """Local validation failed before any authority request."""



def request(message):
    if not isinstance(message, dict):
        raise ValueError('Invalid request')
    operation = message.get('op')
    if operation == 'options':
        command, payload = 'state', {'action': 'list'}
    elif operation == 'save':
        fields = message.get('fields')
        if not isinstance(fields, dict) or fields.get('kind') != 'reading':
            raise ValueError('Only reading captures are supported')
        url = urlsplit(fields.get('reference', ''))
        if url.scheme not in ('http', 'https') or not url.hostname:
            raise ValueError('Reading items need an HTTP or HTTPS URL')
        request_id = str(uuid.UUID(message.get('request_id', '')))
        command, payload = 'request', {'action': 'create', 'request_id': request_id, 'fields': fields}
    else:
        raise ValueError('Unknown Readtjik operation')
    binary = Path.home() / '.local/lib/tasktjik/tasktjik-api'
    if not binary.is_file():
        raise ValueError('Install Tasktjik on this computer first')
    if operation == 'save':
        # The installed model validates fields in memory, without touching the authority.
        # Project membership is checked by the authority against real project IDs.
        probe = {**payload, 'fields': {**fields, 'project': ''}}
        validation = subprocess.run([str(binary), 'request', '--demo'], input=json.dumps(probe),
                                    text=True, capture_output=True, timeout=10, check=False)
        if validation.returncode:
            raise Rejected(validation.stderr.strip() or 'Invalid reading fields')
    result = subprocess.run([str(binary), command], input=json.dumps(payload),
                            text=True, capture_output=True, timeout=35, check=False)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'Tasktjik request failed')
    value = json.loads(result.stdout)
    if operation == 'options':
        # Return only editor choices, never the whole personal task collection.
        return {'groups': value.get('groups', []), 'items': [
            {key: row.get(key) for key in ('id', 'title', 'kind', 'subtasks', 'workflow')}
            for row in value.get('tasks', []) if row.get('kind') in ('project', 'template')
        ]}
    if not isinstance(value, dict) or not value.get('id'):
        raise ValueError('Tasktjik did not acknowledge the save; retry the same capture')
    return {'saved': True, 'id': value['id']}


def read_exact(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.read(size - len(data))
        if not chunk:
            raise EOFError('Incomplete native message')
        data.extend(chunk)
    return bytes(data)


def serve(source, target):
    header = source.read(4)
    if not header:
        return
    if len(header) != 4:
        raise ValueError('Incomplete native header')
    length = struct.unpack('@I', header)[0]
    if length > MAX_MESSAGE:
        raise ValueError('Native message too large')
    try:
        response = request(json.loads(read_exact(source, length)))
    except Exception as error:
        response = {'error': str(error), 'rejected': isinstance(error, Rejected)}
    data = json.dumps(response).encode()
    if len(data) > MAX_MESSAGE:
        data = b'{"error":"Editor choices exceed the browser message limit"}'
    target.write(struct.pack('@I', len(data)) + data)
    target.flush()


if __name__ == '__main__':
    os.umask(0o077)
    serve(sys.stdin.buffer, sys.stdout.buffer)
