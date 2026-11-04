"""Build the public, offline GeoNames city extract; accepts a downloaded ZIP."""
import hashlib
from pathlib import Path
import sys
import zipfile

source = Path(sys.argv[1])
active = {"PPL", "PPLA", "PPLA2", "PPLA3", "PPLA4", "PPLA5", "PPLC", "PPLG", "PPLL"}
territories = {"HK": "Hong Kong", "MO": "Macau", "SG": "Singapore"}
rows = []
with zipfile.ZipFile(source) as archive:
    for line in archive.read("cities15000.txt").decode().splitlines():
        fields = line.split("\t")
        if len(fields) >= 19 and fields[6] == "P" and fields[7] in active:
            name = territories.get(fields[8], fields[1])
            assert name and not any(ord(c) < 32 for c in name)
            assert -90 <= float(fields[4]) <= 90 and -180 <= float(fields[5]) <= 180
            rows.append((name, fields[8], fields[4], fields[5], fields[14]))
rows.sort(key=lambda row: (row[1], row[0], row[2], row[3]))
target = Path(__file__).resolve().parents[1] / "data/cities.tsv"
target.parent.mkdir(exist_ok=True)
target.write_text("# GeoNames cities15000, CC BY 4.0. See docs/geonames-cities.md.\n"
                  "# name\tcountry\tlatitude\tlongitude\tpopulation\n"
                  + "".join("\t".join(row) + "\n" for row in rows))
print(f"{len(rows)} cities; source SHA-256: {hashlib.sha256(source.read_bytes()).hexdigest()}")
