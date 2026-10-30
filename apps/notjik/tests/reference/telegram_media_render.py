"""Bounded, isolated TGS/WebM sticker conversion. Input is a validated TDLib cache file."""
import gzip
import json
import math
from pathlib import Path
import resource
import subprocess
import sys


def render(source, output, format):
    resource.setrlimit(resource.RLIMIT_CPU, (20, 20))
    resource.setrlimit(resource.RLIMIT_FSIZE, (20 * 1024 * 1024,) * 2)
    resource.setrlimit(resource.RLIMIT_AS, (1536 * 1024 * 1024,) * 2)
    base = ['ffmpeg', '-v', 'error', '-nostdin', '-y', '-threads', '1', '-filter_complex_threads', '1']
    palette = 'split[a][b];[a]palettegen=reserve_transparent=1[p];[b][p]paletteuse=alpha_threshold=128'
    if format == 'tgs':
        from rlottie_python import LottieAnimation
        with gzip.open(source, 'rb') as stream:
            raw = stream.read(4 * 1024 * 1024 + 1)
        if len(raw) > 4 * 1024 * 1024:
            raise ValueError('Sticker data too large')
        data = json.loads(raw)
        # Telegram vector stickers need no external files, URLs or embedded images.
        if any(asset.get('p') or asset.get('u') for asset in data.get('assets', [])):
            raise ValueError('External sticker assets are unsupported')
        with LottieAnimation.from_data(json.dumps(data)) as animation:
            rate = float(animation.lottie_animation_get_framerate())
            total = animation.lottie_animation_get_totalframe()
            if not math.isfinite(rate) or rate <= 0 or total <= 0 or total / rate > 10:
                raise ValueError('Invalid sticker duration')
            width, height = animation.lottie_animation_get_size()
            if width <= 0 or height <= 0 or max(width, height) > 4096:
                raise ValueError('Invalid sticker dimensions')
            scale = min(1, 256 / max(width, height)); width = max(1, round(width * scale)); height = max(1, round(height * scale))
            fps = min(30, rate); frames = max(1, math.ceil(total / rate * fps))
            command = base + ['-f', 'rawvideo', '-pixel_format', 'bgra', '-video_size', f'{width}x{height}', '-framerate', str(fps), '-i', 'pipe:0', '-filter_complex', palette, '-loop', '0', '-f', 'gif', str(output)]
            with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as process:
                for frame in range(frames):
                    process.stdin.write(animation.lottie_animation_render(min(total-1, int(frame * rate / fps)), width=width, height=height))
                process.stdin.close()
                if process.wait(timeout=15): raise ValueError('Sticker conversion failed')
    elif format == 'webp':
        subprocess.run(base + ['-i', str(source), '-frames:v', '1', '-vf', 'scale=512:512:force_original_aspect_ratio=decrease', '-f', 'image2', '-update', '1', '-c:v', 'png', str(output)],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
    elif format == 'webm':
        command = base + ['-c:v', 'libvpx-vp9', '-i', str(source), '-t', '10', '-an', '-filter_complex', 'fps=30,scale=256:256:force_original_aspect_ratio=decrease,' + palette, '-loop', '0', '-f', 'gif', str(output)]
        subprocess.run(command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=25)
    else:
        raise ValueError('Unsupported conversion format')


if __name__ == '__main__':
    render(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3])
