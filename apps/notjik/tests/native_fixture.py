"""Private native runtime copies for QML acceptance; artwork stays read-only."""
import os
from pathlib import Path
import shutil

ROOT=Path(__file__).resolve().parents[1]
def copy_file(source,target):
    if Path(source).suffix in ('.qml','.js','.json') or Path(source).name=='qmldir':
        return shutil.copy2(source,target)
    os.symlink(Path(source).resolve(),target)
    return target

def copy_runtime(target):
    for name in ('qml','themes'):
        shutil.copytree(ROOT/name,target/name,copy_function=copy_file,dirs_exist_ok=True)
    (target/'bin').mkdir(exist_ok=True)
    for name in ('notjik-core','notjikctl'):
        shutil.copy2(ROOT/'dist/bin/notjik-core',target/'bin'/name)
    os.environ['NOTJIK_HOME']=str(target)
