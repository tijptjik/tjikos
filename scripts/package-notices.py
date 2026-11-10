"""Bundle, install and verify the source notices shipped by the reviewed GTK apps."""
import argparse
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
COMPONENTS = {
    "bartjik": ("packages/tjik-desktop", "packages/tjik-gtk", "apps/notjik"),
    "notjik": ("packages/tjik-desktop", "packages/tjik-gtk"),
    "runtjik": ("packages/tjik-desktop", "packages/tjik-gtk", "packages/tasktjik-gtk", "apps/tasktjik"),
    "tasktjik": (),
}


def bundle(app, destination):
    files = []
    for name in ("LICENSE", "NOTICE"):
        files.append((ROOT / "apps" / app / name, destination / name))
    for component in COMPONENTS[app]:
        for name in ("LICENSE", "NOTICE"):
            files.append((ROOT / component / name, destination / "licenses" / Path(component).name / name))
    # Check all inputs before producing any part of the bundle.
    for source, _ in files:
        if not source.is_file():
            raise FileNotFoundError(source)
    (destination / "licenses").mkdir(parents=True, exist_ok=True)
    for source, target in files:
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    return len(files)


def legal_files(source):
    for name in ("LICENSE", "NOTICE"):
        path = source / name
        if not path.is_file():
            raise FileNotFoundError(path)
        yield Path(name)
    if not (source / "licenses").is_dir():
        raise FileNotFoundError(source / "licenses")
    for path in sorted((source / "licenses").rglob("*")):
        if path.is_file():
            yield path.relative_to(source)


def install(source, destination):
    files = list(legal_files(source))
    (destination / "licenses").mkdir(parents=True, exist_ok=True)
    for relative in files:
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / relative, target)
    return len(files)


def verify(source, destination):
    files = list(legal_files(source))
    for relative in files:
        if (source / relative).read_bytes() != (destination / relative).read_bytes():
            raise ValueError(f"Notice mismatch: {relative}")
    return len(files)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    build = sub.add_parser("bundle")
    build.add_argument("app", choices=COMPONENTS)
    build.add_argument("destination", type=Path)
    for action in ("install", "verify"):
        command = sub.add_parser(action)
        command.add_argument("source", type=Path)
        command.add_argument("destination", type=Path)
    args = parser.parse_args()
    if args.action == "bundle":
        count = bundle(args.app, args.destination)
    elif args.action == "install":
        count = install(args.source, args.destination)
    else:
        count = verify(args.source, args.destination)
    print(f"{args.action}: {count} license/notice files")


if __name__ == "__main__":
    main()
