# Material Symbols Rounded

The unmodified official TTF is vendored here so Bartjik can register its icon
font with Qt without a system package, font-cache setup or network download.
`../IconFonts.qml` loads it for the application. The build verifies the size
and SHA-256 recorded in `provenance.json`; the upstream Git blob hash is also
recorded for independent verification.

Source: <https://github.com/google/material-design-icons>

License: Apache-2.0; see `LICENSE.txt`. The upstream revision and source path
are pinned in `provenance.json`. To update, choose a new explicit upstream
commit, download that exact TTF, verify its Git blob hash, update the size and
SHA-256, and run the bundled-font render test before committing all changes
together. Do not replace the font from a floating branch or distro package.
