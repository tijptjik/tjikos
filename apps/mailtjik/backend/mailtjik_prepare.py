#!/usr/bin/env python3
"""Prepare an input index without invoking the retired account-wide exporter."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid
from email_archive_run import save_json,source_state,validate_index

def main():
    p=argparse.ArgumentParser();p.add_argument('--workdir',type=Path,required=True);p.add_argument('--config',type=Path,required=True);a=p.parse_args()
    os.umask(0o077);a.workdir.mkdir(parents=True,exist_ok=True,mode=0o700)
    with (a.workdir/'pipeline.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        config=json.loads(a.config.read_text());sources=source_state(config);digest=hashlib.sha256(a.config.read_bytes()).hexdigest()
        marker=a.workdir/'index.complete.json'
        if marker.exists():
            previous=json.loads(marker.read_text())
            if previous['sources']!=sources or previous['config_sha256']!=digest:raise ValueError('Use a new import directory for changed sources or account policy')
            return
        data=a.workdir/('index-'+uuid.uuid4().hex);data.mkdir(mode=0o700)
        save_json(a.workdir/'status.json',{'stage':'indexing_sources'})
        try:
            with (data/'result.json').open('w') as out:
                subprocess.run([sys.executable,str(Path(__file__).with_name('email_archive_index.py')),'--workdir',str(data),'--config',str(a.config)],check=True,stdout=out)
            if sources!=source_state(config):raise ValueError('Source changed during preparation')
            counts=validate_index(data/'email-index.sqlite',config)
            save_json(marker,{'sources':sources,'config_sha256':digest,'data_directory':str(data),'counts':counts})
            save_json(a.workdir/'status.json',{'stage':'index_complete',**counts})
        except Exception as error:
            save_json(a.workdir/'status.json',{'stage':'interrupted','error_type':type(error).__name__});raise
if __name__=='__main__':main()
