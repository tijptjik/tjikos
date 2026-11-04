# Offline device city names

Bartjik embeds `data/cities.tsv`, derived from the
[GeoNames geographical database](https://www.geonames.org/), under the
[Creative Commons Attribution 4.0 license](https://creativecommons.org/licenses/by/4.0/).
Credit: GeoNames and its contributors. The data is provided without warranty.

Source: <https://download.geonames.org/export/dump/cities15000.zip>

Downloaded: 2026-11-04. Source ZIP SHA-256:
`1d7cf2e7b0920b5203a5c0f2b3825e5e5d6e721e3802593cb51b438877796847`

The source includes populated places with over 15,000 inhabitants and capitals.
This extract retains active populated-place feature codes `PPL`, `PPLA`, `PPLA2`,
`PPLA3`, `PPLA4`, `PPLA5`, `PPLC`, `PPLG` and `PPLL`. City sections, historic and
abandoned settlements are excluded. The retained columns are name, country code,
latitude, longitude and population. Localities in Hong Kong, Macau and Singapore
use their city-wide name. Rows are sorted by country, name, latitude and longitude.

The extract contains 31,764 points. Bartjik uses great-circle distance and prefers
the closest major city (at least one million inhabitants) within 25 km, so a
metropolitan location shows the city rather than a small ward or housing district.
Otherwise it uses the closest populated place within 50 km. The panel says
**Near …** because city points cannot establish municipal boundaries. If there is
no point within 50 km, it shows device coordinates. City naming is entirely
offline; the original device fix, its age and the solar calculation are unchanged.

To reproduce or update the extract, download the ZIP and run
`python3 scripts/update-cities.py /path/to/cities15000.zip` from the Bartjik
directory. Update this document's download date, source checksum and record count
when changing the dataset.
