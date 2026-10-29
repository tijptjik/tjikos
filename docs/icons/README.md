# README icons

These self-contained SVGs are copies of the Rose Punk app icons from the
Dotjik policy repository. Keeping them here makes the README work in a
standalone TjikOS checkout and on GitHub.

Canonical artwork: `scripts/icons/local/` in the Dotjik repository.
Rendered exports: `native/home/files/.local/share/icons/rose-punk/scalable/apps/`.
After changing the source, run `python3 scripts/icons/release.py build` there and copy
these seven exports here again.

Mailtjik uses the shared 40-unit rounded-square identity with a dark inset,
white, blue and rose-red airmail dashes, and a white envelope flap. Notjik has a
small rounded-square center; Runtjik has a smaller pale pink Command symbol
inside a periwinkle frame. All artwork is editable SVG, with no
external resources or font dependencies.

Bartjik, Notjik, Runtjik, Mailtjik and Tjikup are original themed artwork.
Pony is original fan artwork inspired by Rainbow Dash; character design rights
belong to Hasbro. It is not an official project logo.

README headings use 96 px PNG exports displayed at 32 px, with top alignment
to keep every icon on the same heading line. SVG remains the editable source.
Regenerate an export with `magick -background none -density 192 NAME.svg -resize 96x96 NAME.png`.
