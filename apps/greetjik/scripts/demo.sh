#!/usr/bin/env bash
# Synthetic, deterministic screenshot fixture. No user files or shell config.
set -euo pipefail
if [[ "${1:-}" != --demo ]]; then
    printf 'Usage: %s --demo\n' "$0" >&2
    exit 2
fi
printf '\033[2J\033[H\033[?25l'
printf '\033[1;38;5;141mgreetjik\033[0m  \033[38;5;245mponies, at native speed\033[0m\n\n'
printf '\033[38;5;117m$\033[0m printf '\''A hundred ponies a day. Every millisecond counts.\\n'\'' \\\n'
printf '    | greetjik -f twilight\n\n'
printf 'A hundred ponies a day. Every millisecond counts.\n' | "${GREETJIK_BIN:-greetjik}" -f twilight
printf '\n\033[38;5;245m474 ponies + 103 extras  /  piped quotes  /  one native binary\033[0m\n'
