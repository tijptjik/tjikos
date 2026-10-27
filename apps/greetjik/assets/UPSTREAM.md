# Artwork provenance

Source: https://github.com/erkin/ponysay
Pinned commit: `00b8c84d2fff682bc1a02221d92e798cc60d58ea`
Archive: https://codeload.github.com/erkin/ponysay/tar.gz/00b8c84d2fff682bc1a02221d92e798cc60d58ea
Archive SHA-256: `ef7258fad30f817ac6396f370069f4ce9e1041466e76e3ebee7399c8f70e3b28`

The `ponies`, `extraponies`, `ttyponies`, and `extrattyponies` directories are
unmodified upstream artwork, including aliases and metadata. There are 474
regular and 103 extra pony names, with both terminal and TTY variants (1,154
entries). Artwork is embedded at build time. No artwork is fetched at runtime.

The renderer follows upstream balloon and template semantics. Program code is
GPL-3.0-or-later; see ../LICENSE. Upstream's separate artwork licensing terms
are retained in UPSTREAM-LICENSE, individual pony headers, and CREDITS. Some
artwork restricts commercial redistribution; embedding it does not change its
license. Preserve these notices and metadata with future distributions.

To refresh: download a reviewed, pinned commit; verify the archive SHA-256;
replace the four directories without modifying pony headers or aliases; update
this record and the catalogue-count tests; run the full tests and the optional
upstream differential check. Do not silently track upstream master.
