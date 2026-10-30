"""On-demand API-v1 provider/action host for the native launcher.

Discovery, ranking, history reads and the normal query loop belong to Rust.
This host preserves existing Python extensions and complex desktop actions.
"""
import json
import sys
import runtjik as r


class HostLauncher(r.Launcher):
    def refresh(self, force=False):
        self.checked = r.time.monotonic()


def main():
    launcher = None
    for line in sys.stdin:
        request = {}
        try:
            request = json.loads(line)
            if launcher is None:
                launcher = HostLauncher(request.get('demo', False))
            if request.get('settings') != launcher.settings:
                launcher = HostLauncher(request.get('demo', False))
            launcher.entries = [r.Entry(**row) for row in request.get('entries', [])]
            # Keep the reference configuration's insertion order for /config.
            if request.get('reset_scope') and 'Browser' in launcher.providers:
                launcher.providers['Browser'].window_scope = None
            operation = request['op']
            if operation == 'activate':
                entry = request.get('entry')
                if entry:
                    launcher.current[entry['id']] = r.Entry(**entry)
                value = launcher.activate(request['id'], request.get('mode', 'launch'),
                                          request.get('address', ''), request.get('prepare', False))
            elif operation == 'app-action':
                value = launcher.app_action(request)
            elif operation == 'undo-trash':
                from desktop_plugins import trash_action
                action = {'message': 'Demo mode leaves Trash unchanged'} if launcher.demo else trash_action('undo')
                value = {**launcher.query(request.get('query', '')), 'message': action['message']}
            elif operation == 'general':
                entries = []
                for name, provider in launcher.providers.items():
                    if name not in ('apps', 'commands'):
                        for row in provider.query(request.get('query', ''), launcher.settings):
                            entry = row if isinstance(row, r.Entry) else r.Entry(**row)
                            if entry.provider != name or not entry.id.startswith(name + ':'):
                                raise ValueError('Provider result ID must be namespaced')
                            entries.append(r.asdict(entry))
                            launcher.current[entry.id] = entry
                value = {'entries': entries, 'warnings': launcher.warnings}
            else:
                value = launcher.query(request.get('query', ''), request.get('offset', 0), request.get('limit'))
            print(json.dumps({**value, 'serial': request.get('serial', 0)}), flush=True)
        except Exception as exc:
            print(json.dumps({'kind': 'error', 'message': str(exc), 'serial': request.get('serial', 0)}), flush=True)


if __name__ == '__main__':
    main()
