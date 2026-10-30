#!/usr/bin/env bash
# TjikOS cold bootstrap for a fresh Fedora x86_64 installation.
# Review: curl -fsSLo init.sh https://sh.type.hk/init; less init.sh; bash init.sh
# This only bootstraps and enrolls. It does not run the selected recipe graph.
set -euo pipefail

main() {
    local mode="" answer dotjik_dir tjikos_dir config_dir
    while (( $# )); do
        case "$1" in
            --help|--plan) mode="$1"; shift ;;
            *) echo "Unknown bootstrap option: $1" >&2; return 2 ;;
        esac
    done
    local -a packages=(ca-certificates curl git fish gum age jq rclone)
    if [[ "$mode" == --help || "$mode" == --plan ]]; then
        printf '%s\n' 'TjikOS Fedora bootstrap:' \
            '  1. Verify Fedora x86_64 and a normal sudo-capable user.' \
            "  2. Install Fedora packages: ${packages[*]}" \
            '  3. Fetch pinned public sources over HTTPS; no GitHub credentials required.' \
            '  4. Download pinned static tjikup and synctjik releases; verify SHA-256; no compilation.' \
            '  5. Enroll this hostname: role and explicit recipe subscriptions.' \
            '  6. Unlock the existing encrypted Bitwarden token and age identity.' \
            '  7. Validate native template context.' \
            '  8. Select required archive collections, review restoration, and verify readiness.' \
            '  9. Enable scheduled backups after restoration; print the plan. Applying recipes is a separate command.'
        return 0
    fi
    [[ -z "$mode" ]] || { echo 'Usage: bash init.sh [--plan | --help]' >&2; return 2; }
    [[ $EUID -ne 0 ]] || { echo 'Run as your normal user, not root or sudo bash.' >&2; return 1; }
    # Values below are pinned by the release build, never taken from the network
    # during execution of a script you have already reviewed.
    local binary_url='@TJIKUP_BINARY_URL@' binary_sha256='@TJIKUP_BINARY_SHA256@'
    local synctjik_url='@SYNCTJIK_BINARY_URL@' synctjik_sha256='@SYNCTJIK_BINARY_SHA256@'
    local dotjik_revision='@DOTJIK_REVISION@' tjikos_revision='@TJIKOS_REVISION@'
    local dotjik_branch='@DOTJIK_BRANCH@' tjikos_branch='@TJIKOS_BRANCH@'
    [[ "$dotjik_revision" =~ ^[0-9a-f]{40}$ && "$tjikos_revision" =~ ^[0-9a-f]{40}$ && "$binary_sha256" =~ ^[a-f0-9]{64}$ && "$synctjik_sha256" =~ ^[a-f0-9]{64}$ ]] || {
        echo 'This source script is not a published release. Use a built /init artifact.' >&2; return 1;
    }
    [[ -r /etc/os-release ]] || { echo 'Cannot identify the operating system.' >&2; return 1; }
    # /etc/os-release is owned by the installed operating system.
    . /etc/os-release
    [[ "${ID:-}" == fedora && "$(uname -m)" == x86_64 ]] || {
        echo 'This release supports Fedora x86_64; Bitwarden assets are architecture-specific.' >&2; return 1;
    }
    command -v sudo >/dev/null || { echo 'Install sudo and add this user to wheel first.' >&2; return 1; }
    [[ -r /dev/tty ]] || { echo 'An interactive terminal is required for enrollment and credential unlock.' >&2; return 1; }
    printf '%s\n' 'This installs bootstrap packages and prepares your device. It does not apply dotfiles or services.'
    printf 'Continue? [y/N]: ' >/dev/tty
    read -r answer </dev/tty
    [[ "$answer" == y || "$answer" == Y ]] || return 0
    sudo -v </dev/tty
    sudo dnf install -y "${packages[@]}" </dev/tty
    umask 077
    dotjik_dir="$HOME/code/dotjik"
    tjikos_dir="$HOME/code/tjikos"
    config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/tjikup"
    clone_release() {
        local url="$1" target="$2" revision="$3" branch="$4" origin
        if [[ -e "$target" || -L "$target" ]]; then
            [[ -d "$target/.git" ]] || { echo "Refusing existing non-checkout: $target" >&2; return 1; }
            origin="$(git -C "$target" remote get-url origin)"
            [[ "$origin" == "$url" || "$origin" == "git@github.com:${url#https://github.com/}" ]] || {
                echo "Unexpected origin for $target; inspect it before continuing." >&2; return 1;
            }
            [[ -z "$(git -C "$target" status --porcelain)" ]] || { echo "Preserve local changes in $target before bootstrapping." >&2; return 1; }
            [[ "$(git -C "$target" rev-parse HEAD)" == "$revision" ]] || {
                echo "Existing checkout is not the reviewed release: $target. No reset or checkout was performed." >&2; return 1;
            }
        else
            mkdir -p -- "$(dirname -- "$target")"
            git clone --no-checkout -- "$url" "$target"
            git -C "$target" checkout -b bootstrap "$revision"
            git -C "$target" branch --set-upstream-to="origin/$branch" bootstrap
        fi
    }
    clone_release https://github.com/tijptjik/dotfiles.git "$dotjik_dir" "$dotjik_revision" "$dotjik_branch"
    clone_release https://github.com/tijptjik/tjikos.git "$tjikos_dir" "$tjikos_revision" "$tjikos_branch"
    [[ -f "$tjikos_dir/apps/tjikup/Cargo.toml" && -f "$dotjik_dir/native/home/bootstrap.json" ]] || {
        echo 'Pinned source does not contain the cold-bootstrap workflow.' >&2; return 1;
    }
    mkdir -p "$HOME/.local/bin" "$config_dir"
    bootstrap_binary_tmp="$(mktemp "$HOME/.local/bin/.tjikup.XXXXXX")"
    trap 'rm -f -- "${bootstrap_binary_tmp:-}"' EXIT
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 15 --max-time 180 "$binary_url" -o "$bootstrap_binary_tmp"
    [[ "$(sha256sum "$bootstrap_binary_tmp" | cut -d ' ' -f 1)" == "$binary_sha256" ]] || {
        echo 'Runner checksum mismatch; existing executable preserved.' >&2; return 1;
    }
    chmod 755 "$bootstrap_binary_tmp"
    "$bootstrap_binary_tmp" --version >/dev/null
    "$bootstrap_binary_tmp" capabilities | jq -e '
        .native_workflow == 1 and .bootstrap_archives == 1 and
        .native_commands == 1 and .native_installers == 1 and .native_maintenance == 1 and
        .scheduled_telegram >= 1 and .scheduled_telegram_bws >= 1
    ' >/dev/null || {
        echo 'Pinned Tjikup release lacks the required native workflows. Publish a compatible release and update pins; existing executable preserved.' >&2
        return 1
    }
    mv -f -- "$bootstrap_binary_tmp" "$HOME/.local/bin/tjikup"
    bootstrap_binary_tmp="$(mktemp "$HOME/.local/bin/.synctjik.XXXXXX")"
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 15 --max-time 180 "$synctjik_url" -o "$bootstrap_binary_tmp"
    [[ "$(sha256sum "$bootstrap_binary_tmp" | cut -d ' ' -f 1)" == "$synctjik_sha256" ]] || {
        echo 'Synctjik checksum mismatch; existing executable preserved.' >&2; return 1;
    }
    chmod 755 "$bootstrap_binary_tmp"
    "$bootstrap_binary_tmp" --version >/dev/null
    mv -f -- "$bootstrap_binary_tmp" "$HOME/.local/bin/synctjik"
    export PATH="$HOME/.local/bin:/usr/local/bin:$PATH"
    if [[ -e "$config_dir/config.json" || -L "$config_dir/config.json" ]]; then
        [[ "$(readlink -f "$config_dir/config.json")" == "$dotjik_dir/tjikup.json" ]] || {
            echo "Refusing to replace existing $config_dir/config.json" >&2; return 1;
        }
    else
        ln -s "$dotjik_dir/tjikup.json" "$config_dir/config.json"
    fi
    if [[ ! -f "$config_dir/device.json" ]]; then
        tjikup new device </dev/tty
    fi
    tjikup --check
    bash "$dotjik_dir/scripts/install-tjikup.sh"
    tjikup workflow --repo "$dotjik_dir" bitwarden
    fish --no-config "$dotjik_dir/native/tasks/bootstrap/setup-bitwarden.fish" </dev/tty
    age-keygen -y "$HOME/.keys/age.txt" >/dev/null
    tjikup workflow --repo "$dotjik_dir" configure
    bash "$tjikos_dir/apps/tjikos-sh/helpers/restore-collections.sh" "$dotjik_dir" </dev/tty
    tjikup --check
    tjikup --plan
    printf '\n%s\n' 'Bootstrap and required archive restoration complete. Review the plan, then run: tjikup --skip-repo-sync --show-skips' \
        'Public repositories use HTTPS. Configure write access separately if this device needs to push.' \
        'Device guide: https://sh.type.hk/guide'
}
main "$@"
