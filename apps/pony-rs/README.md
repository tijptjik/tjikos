# pony-rs

![pony-rs rendering Twilight with a piped quote](docs/showcase.png)

Pony artwork and piped quotes, rendered by a small native Rust executable.
The complete upstream ponysay artwork catalogue is embedded in one binary.

```sh
fortune | pony-rs
```

Each invocation selects a random pony and renders the text supplied on stdin.
Use `-f NAME` to choose a particular pony.

## Motivation

We run this a hundred times a day. Startup latency matters: a small pause on
every invocation adds up. pony-rs keeps the ponies and the quotes while
minimizing the work between input and output.

The original Python ponysay also brought packaging and compatibility friction.
Its upstream issue history documents specific examples:

| Area | Reported problem |
| --- | --- |
| Python packaging | The custom installer tried to write to `/usr/bin` even inside a virtualenv, causing permission errors ([#176](https://github.com/erkin/ponysay/issues/176)). Standard pip installation was requested separately ([#248](https://github.com/erkin/ponysay/issues/248)). |
| Installation dependencies and paths | Missing `makeinfo`/Texinfo caused a Fedora build to fail while generating documentation ([#207](https://github.com/erkin/ponysay/issues/207)). A later macOS report hit protected `/usr/share/info` paths despite choosing user-local executable paths ([#320](https://github.com/erkin/ponysay/issues/320)). |
| Newer Python versions | Python 3.12 exposed invalid escape sequences in balloon docstrings, producing repeated warnings on macOS and Arch Linux ([#314](https://github.com/erkin/ponysay/issues/314), [#317](https://github.com/erkin/ponysay/issues/317)). Those reports describe working rendering with warnings, rather than failed rendering. |

These reports span different releases and systems; some installation issues
were later closed. They motivate a simpler deployment model.

Containerizing ponysay isolates host packaging dependencies but adds container
startup to each invocation. `pony-rs` keeps the artwork and text-rendering core
in a native executable, with no Python runtime, container, resident service, or
runtime asset downloads.

## Speed compared with Docker

Measured on the same Linux x86-64 machine on 2026-10-27, with the same `twilight`
pony and the same piped quote:

| Quote-to-output pipeline | Median | Mean | Range |
| --- | ---: | ---: | ---: |
| Old `xargs -0 docker run --rm …` pipeline | 479.20 ms | 477.33 ms | 451.77–503.31 ms |
| Native `pony-rs` | **1.27 ms** | **1.34 ms** | **1.05–2.05 ms** |

The native pipeline was **about 378x faster by median elapsed time**, removing
approximately **478 ms per invocation** in this comparison.

The benchmark ran two warm-ups and 12 measured invocations per implementation,
alternating their order. Docker was already running, its image was already
present, and `--pull=never` excluded downloads. Each Docker sample still started
and removed a fresh container. Both commands received the same text on stdin; `xargs -0` passed it as an argument to containerized
ponysay. Output was captured through completion and checked for the quote and
artwork. Fortune generation and screen presentation are outside this timer.
These are warm-cache measurements on one machine.

### Reproduce the comparison

Requires Python 3 for the benchmark runner and an already-running Docker daemon
with the old image cached. Docker and Python are not runtime requirements for
pony-rs itself.

```sh
# From apps/pony-rs, after building:
python3 scripts/benchmark.py \
  --binary ./dist/pony-rs \
  --runs 12 --warmups 2 \
  --output /tmp/pony-benchmark.json
```

The runner deliberately does not pull images. The [recorded results](docs/benchmarks/2026-10-27-docker-vs-rust.json)
include every sample, command arguments, platform, binary version, and Docker
image identity.

## Usage

```sh
fortune | pony-rs                             # new quote and random regular pony
printf 'Hello, world!\n' | pony-rs -f twilight
pony-rs -f rainbowdash 'Stay awesome'
pony-rs --list                               # 474 regular entries
pony-rs --all --list                         # all 577 entries
pony-rs --extra 'Hello from the extras'       # random extra pony
pony-rs --extra -f applejack 'Extra variant'  # resolve a shared name explicitly
pony-rs --tty -f twilight 'TTY artwork'
fortune | pony-rs --width 100
```

| Option | Behavior |
| --- | --- |
| `-f`, `--pony NAME` | Choose a pony; `.pony` suffix is optional. |
| `-l`, `--list` | List pony names in the selected collection. |
| `-a`, `--all` | Include extras in random selection and listing. |
| `-A`, `--extra` | Select only extras, including when resolving shared names. |
| `--tty` | Use the corresponding upstream Linux TTY artwork. |
| `-W`, `--width N` | Wrap quote text to the selected terminal width, 12–4096 columns. |
| `-V`, `--version` | Print the application version. |
| `-h`, `--help` | Show usage. |

With no message arguments, pony-rs reads UTF-8 text from stdin. With arguments,
it joins them into the message; use `--` before text beginning with a dash.
Running without text in an interactive terminal prints usage guidance instead
of waiting for input. Unknown pony names and invalid options return an error.

`TERM=linux` automatically selects TTY artwork. Width defaults to the actual
terminal dimensions, then `COLUMNS`, then 80. Multiline quotes and Unicode
column widths are supported. Terminal control codes are removed from quoted
text; artwork retains its ANSI colours.

## Coverage and scope

The binary embeds **474 regular ponies and 103 extras**, each with a TTY variant:
**1,154 artwork entries**, including upstream aliases and metadata. It does not
need a runtime artwork directory or network connection.

The supported scope is pony selection, artwork rendering, and piped or argument
text. Custom balloon themes, image conversion, palette/KMS administration, and
the built-in episode-quote database are omitted. Pipe `fortune` or any other
text source into pony-rs to supply quotes.

All 1,154 entries render in the catalogue tests. Differential tests match 1,152
upstream layouts. Upstream's `ttyponies/twilightrage` raises an exception, and
`extrattyponies/velvetremedy` corrupts escape sequences around its embedded
balloon. Dedicated Rust tests verify the corrected TTY layouts against their
regular-artwork counterparts rather than reproducing those bugs.

## Build and install

Building requires Rust and Cargo. Runtime dependencies are supplied by the
compiled binary and system libraries; the two locked Rust crates are `libc`
and `unicode-width`. Install `fortune-mod` separately if you want random fortunes.

From `apps/pony-rs`:

```sh
bash scripts/build.sh
bash scripts/install.sh
```

Or from the tooltjik workspace root:

```sh
bun install --frozen-lockfile
bun run build --filter=@tooltjik/pony-rs
bash scripts/install.sh pony-rs
```

Installation places the executable in `~/.local/bin/pony-rs` and license notices
in `~/.local/share/licenses/pony-rs`. Add `~/.local/bin` to PATH. The direct Cargo
build and install scripts do not require Bun.

## Development checks

```sh
# From apps/pony-rs:
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check

# From the tooltjik root:
bun run check --filter=@tooltjik/pony-rs

# Optional comparison with a checkout of the pinned upstream:
python3 scripts/check-parity.py /path/to/erkin-ponysay
```

Tests cover catalogue completeness, stdin, selection, duplicate names, Unicode,
ANSI colour restoration, multiline wrapping, and embedded speech bubbles.

## Artwork provenance and licensing

Artwork is pinned to erkin/ponysay commit
`00b8c84d2fff682bc1a02221d92e798cc60d58ea`. See
[assets/UPSTREAM.md](assets/UPSTREAM.md) for the source archive, checksum, update
procedure, and attribution. Program code is GPL-3.0-or-later. Artwork retains
its individual upstream terms, including restrictions on commercial
redistribution for some images; embedding it does not change those terms.
