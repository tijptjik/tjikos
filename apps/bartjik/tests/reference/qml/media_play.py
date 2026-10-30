"""Start Spotify if needed, then resume its current playback context."""
import shutil
import subprocess
import sys
import time


def spotify_running():
    result = subprocess.run(['playerctl', '-l'], capture_output=True, text=True, timeout=5)
    return any(name == 'spotify' or name.startswith('spotify.') for name in result.stdout.splitlines())


def play():
    if not spotify_running():
        command = ['spotify'] if shutil.which('spotify') else ['flatpak', 'run', 'com.spotify.Client']
        subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         start_new_session=True)
    for _ in range(60):
        if spotify_running():
            subprocess.run(['playerctl', '--player=spotify', 'play'], check=True, timeout=5)
            return
        time.sleep(0.5)
    raise RuntimeError('Spotify did not become available for playback within 30 seconds')


if __name__ == '__main__':
    try:
        play()
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
