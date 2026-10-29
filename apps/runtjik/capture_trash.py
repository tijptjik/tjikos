"""Use the desktop's Trash, keeping a private undo stack for Runtjik actions."""
import fcntl
import json
import os
from pathlib import Path
import sys
import tempfile


def state_path():
    root = Path(os.environ.get('XDG_STATE_HOME', str(Path.home() / '.local/state'))) / 'runtjik'
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    return root / 'trash-undo.json'


def trash_items(Gio):
    root = Gio.File.new_for_uri('trash:///')
    entries = root.enumerate_children('standard::name,trash::orig-path', Gio.FileQueryInfoFlags.NOFOLLOW_SYMLINKS, None)
    result = {}
    try:
        while True:
            info = entries.next_file(None)
            if info is None:
                break
            result[root.get_child(info.get_name()).get_uri()] = info.get_attribute_byte_string('trash::orig-path')
    finally:
        entries.close(None)
    return result


def action(operation, path=None):
    import gi
    from gi.repository import Gio
    state = state_path()
    with state.with_suffix('.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        stack = json.loads(state.read_text()) if state.exists() else []
        if operation == 'trash':
            target = Path(path)
            if not target.is_absolute() or not target.is_file() or target.is_symlink():
                raise ValueError('Capture is no longer available')
            before = trash_items(Gio)
            Gio.File.new_for_path(str(target)).trash(None)
            after = trash_items(Gio)
            matches = [uri for uri, original in after.items() if uri not in before and original == str(target)]
            if len(matches) != 1:
                raise RuntimeError('File is in the desktop Trash, but its undo entry could not be identified; restore it with the file manager')
            stack.append({'uri': matches[0], 'path': str(target)})
            message = 'Moved to Trash: ' + target.name + ' · Ctrl+Z to undo'
        elif operation == 'undo':
            if not stack:
                return {'message': 'No capture trash action to undo'}
            entry = stack[-1]
            target = Path(entry['path'])
            if target.exists() or target.is_symlink():
                raise ValueError('Cannot restore: a file already exists at ' + str(target))
            available = trash_items(Gio)
            if available.get(entry['uri']) != entry['path']:
                raise ValueError('That item is no longer in Trash')
            target.parent.mkdir(parents=True, exist_ok=True)
            Gio.File.new_for_uri(entry['uri']).move(Gio.File.new_for_path(str(target)), Gio.FileCopyFlags.NONE, None, None, None)
            stack.pop()
            message = 'Restored: ' + target.name
        else:
            raise ValueError('Unknown trash operation')
        fd, temporary = tempfile.mkstemp(dir=state.parent, prefix='.trash-undo-')
        try:
            with os.fdopen(fd, 'w') as stream:
                json.dump(stack, stream)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, state)
        finally:
            Path(temporary).unlink(missing_ok=True)
        return {'message': message}


if __name__ == '__main__':
    try:
        print(json.dumps(action(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)))
    except Exception as exc:
        print(json.dumps({'error': str(exc)}))
        sys.exit(1)
