"""Open imv at an image-proportional, centered Hyprland lightbox size."""
import argparse
import configparser
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def lightbox_size(image_width, image_height, screen_width, screen_height):
    height = max(1, round(screen_height * .9))
    margin = (screen_height - height) / 2
    width = max(1, min(round(height * image_width / image_height), round(screen_width - 2 * margin)))
    return width, height


def open_images(paths, viewer=None):
    viewer = viewer or ['imv']
    paths = [str(Path(path).expanduser().resolve()) for path in paths]
    if not paths:
        return subprocess.call(viewer)
    argv = [*viewer, '-n', paths[0], *(paths if len(paths) > 1 else [str(Path(paths[0]).parent)])]
    try:
        monitors = json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], timeout=3))
        monitor = next((m for m in monitors if m.get('focused')), monitors[0])
        width, height = monitor['width'], monitor['height']
        if monitor.get('transform', 0) % 2:
            width, height = height, width
        scale = monitor.get('scale', 1)
        dimensions = subprocess.check_output(['magick', 'identify', '-ping', '-format', '%w %h', paths[0] + '[0]'], timeout=5).decode().split()
        size = lightbox_size(int(dimensions[0]), int(dimensions[1]), width / scale, height / scale)
    except (OSError, ValueError, KeyError, IndexError, subprocess.SubprocessError):
        return subprocess.call(argv)
    config = configparser.ConfigParser(interpolation=None, strict=False)
    config.optionxform = str
    home = Path.home()
    sources = [os.environ.get('imv_config', ''),
               str(Path(os.environ.get('XDG_CONFIG_HOME', str(home / '.config'))) / 'imv/config'),
               str(home / '.imv_config'), str(home / '.imv/config'), '/usr/local/etc/imv_config', '/etc/imv_config']
    source = next((path for path in sources if path and Path(path).is_file()), None)
    if source:
        config.read(source)
    if not config.has_section('options'):
        config.add_section('options')
    config['options'].update(width=str(size[0]), height=str(size[1]), scaling_mode='full')
    with tempfile.TemporaryDirectory(prefix='imv-lightbox-', dir=os.environ.get('XDG_RUNTIME_DIR')) as directory:
        path = Path(directory) / 'config'
        with path.open('w') as stream:
            config.write(stream)
        process = subprocess.Popen(argv, env={**os.environ, 'imv_config': str(path)})
        # imv can request its final size after the compositor's initial centering.
        # Address this instance only once its surface has mapped.
        try:
            for _ in range(40):
                if process.poll() is not None:
                    break
                clients = json.loads(subprocess.check_output(['hyprctl', '-j', 'clients'], timeout=2))
                client = next((c for c in clients if c.get('pid') == process.pid and c.get('mapped')), None)
                if client:
                    monitors = json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], timeout=2))
                    monitor = next(m for m in monitors if m['id'] == client['monitor'])
                    width, height = monitor['width'], monitor['height']
                    if monitor.get('transform', 0) % 2:
                        width, height = height, width
                    scale = monitor.get('scale', 1)
                    size = lightbox_size(int(dimensions[0]), int(dimensions[1]), width / scale, height / scale)
                    time.sleep(.15)
                    target = json.dumps('address:' + client['address'])
                    x = round(monitor['x'] + (width / scale - size[0]) / 2)
                    y = round(monitor['y'] + (height / scale - size[1]) / 2)
                    for command in [f'hl.dsp.window.resize({{x={size[0]},y={size[1]},relative=false,window={target}}})',
                                    f'hl.dsp.window.move({{x={x},y={y},relative=false,window={target}}})']:
                        subprocess.run(['hyprctl', 'dispatch', command], check=True, stdout=subprocess.DEVNULL, timeout=2)
                    break
                time.sleep(.05)
        except (OSError, ValueError, KeyError, subprocess.SubprocessError):
            pass  # Keep the viewer usable if the compositor is reloading.
        return process.wait()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--viewer', type=json.loads, default=['imv'])
    parser.add_argument('paths', nargs='*')
    args = parser.parse_args()
    raise SystemExit(open_images(args.paths, args.viewer))
