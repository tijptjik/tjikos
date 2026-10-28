#!/usr/bin/env bash
# Only the new machine runs this helper. Tokens never enter Git config or argv.
set -euo pipefail
umask 077
key_secret="${TJIKUP_SSH_KEY_SECRET_ID:-}"
if [[ ${1:-} == --ssh-key-secret && $# == 2 ]]; then
    key_secret="$2"
elif [[ $# != 0 ]]; then
    echo 'Usage: github-ssh.sh [--ssh-key-secret BWS_SECRET_ID]' >&2; exit 2
fi
[[ -z "$key_secret" || "$key_secret" =~ ^[a-fA-F0-9-]{36}$ ]] || { echo 'Invalid BWS secret ID' >&2; exit 2; }
ssh_dir="$HOME/.ssh"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/tjikup"
mkdir -p "$ssh_dir" "$config_dir"
chmod 700 "$ssh_dir"
key="$ssh_dir/tjikup_github_ed25519"
ssh_config="$ssh_dir/tjikup-github.conf"
known_hosts="$ssh_dir/tjikup-github-known-hosts"
[[ ! -L "$key" && ! -L "$ssh_config" && ! -L "$known_hosts" ]] || {
    echo 'Refusing symlinks at managed GitHub SSH paths.' >&2; exit 1;
}
temporary="$(mktemp -d "$ssh_dir/.tjikup-github.XXXXXX")"
trap 'rm -rf -- "$temporary"; unset github_token BWS_ACCESS_TOKEN' EXIT
github_token=''
public_api() {
    curl --proto '=https' --tlsv1.2 -fsS --retry 2 --connect-timeout 15 --max-time 45 \
        -H 'Accept: application/vnd.github+json' -H 'User-Agent: tjikos-bootstrap' "https://api.github.com/$1"
}
authenticated_api() {
    local method="$1" path="$2"
    shift 2
    # The secret header is provided on stdin, never in the process command line.
    printf 'header = "Authorization: Bearer %s"\n' "$github_token" |
        curl --config - --proto '=https' --tlsv1.2 -fsS --connect-timeout 15 --max-time 45 \
            -H 'Accept: application/vnd.github+json' -H 'User-Agent: tjikos-bootstrap' \
            -X "$method" "https://api.github.com/$path" "$@"
}
load_bws() {
    local line
    if [[ -z ${BWS_ACCESS_TOKEN:-} ]]; then
        while IFS= read -r line || [[ -n "$line" ]]; do
            if [[ "$line" == BWS_ACCESS_TOKEN=* ]]; then
                export BWS_ACCESS_TOKEN="${line#BWS_ACCESS_TOKEN=}"
            fi
        done < "$HOME/.config/bws/environment"
    fi
    [[ -n ${BWS_ACCESS_TOKEN:-} ]] || { echo 'Unlock BWS before GitHub SSH setup.' >&2; exit 1; }
}
verify_ssh() {
    local code=0
    ssh -F "$ssh_config" -T git@github.com > "$temporary/ssh-result" 2>&1 || code=$?
    [[ "$code" == 1 ]] && grep -Fq "Hi tijptjik! You've successfully authenticated" "$temporary/ssh-result"
}
public_api meta | jq -er '.ssh_keys[] | "github.com " + .' > "$temporary/known-hosts"
ssh-keygen -l -f "$temporary/known-hosts" >/dev/null
install -m 600 "$temporary/known-hosts" "$known_hosts"
# Quote filesystem paths for OpenSSH config syntax, not shell evaluation.
quote_ssh() { local value="${1//\\/\\\\}"; printf '"%s"' "${value//\"/\\\"}"; }
{
    printf 'Host github.com\n  HostName github.com\n  User git\n  BatchMode yes\n  IdentitiesOnly yes\n  StrictHostKeyChecking yes\n  IdentityFile '
    quote_ssh "$key"
    printf '\n  UserKnownHostsFile '
    quote_ssh "$known_hosts"
    printf '\n'
} > "$temporary/config"
install -m 600 "$temporary/config" "$ssh_config"
public_api users/tijptjik | jq -e 'select(.login == "tijptjik") | {login, id, name}' > "$temporary/identity.json"
install -m 600 "$temporary/identity.json" "$config_dir/github.json"
if [[ -f "$key" ]] && verify_ssh; then
    echo 'GitHub SSH: existing device key verified.'
    exit 0
fi
load_bws
if [[ -n "$key_secret" ]]; then
    bws secret get "$key_secret" | jq -er '.value | select(type == "string" and length > 0)' > "$temporary/key"
    chmod 600 "$temporary/key"
    ssh-keygen -y -P '' -f "$temporary/key" > "$temporary/key.pub"
    if [[ -e "$key" ]]; then
        ssh-keygen -y -P '' -f "$key" > "$temporary/existing.pub"
        cmp -s "$temporary/existing.pub" "$temporary/key.pub" || {
            echo 'Existing device key differs from BWS; no key was replaced.' >&2; exit 1;
        }
    else
        ln "$temporary/key" "$key"
    fi
else
    github_token="$(bws secret get 62946f86-1fef-4535-9b9d-b3b000754acd | jq -er '.value | select(type == "string" and length > 0)')"
    [[ "$github_token" =~ ^[A-Za-z0-9_-]+$ ]] || { echo 'Invalid GitHub token format in BWS.' >&2; exit 1; }
    if ! authenticated_api GET user > "$temporary/user.json"; then
        echo 'GHI_TOKEN in BWS was rejected by GitHub. Update it, or use --ssh-key-secret with an existing authorized SSH key.' >&2
        exit 1
    fi
    jq -e '.login == "tijptjik"' "$temporary/user.json" >/dev/null || {
        echo 'BWS GitHub token belongs to a different account; no key was registered.' >&2; exit 1;
    }
    if [[ ! -e "$key" ]]; then
        ssh-keygen -q -t ed25519 -N '' -C "tjikup@$(hostname)" -f "$temporary/key"
        ln "$temporary/key" "$key"
    fi
    chmod 600 "$key"
    ssh-keygen -y -P '' -f "$key" > "$temporary/key.pub"
    read -r key_type key_data _ < "$temporary/key.pub"
    public_key="$key_type $key_data"
    # Compare only type and key data: GitHub may omit the local comment.
    found=false
    for page in {1..20}; do
        authenticated_api GET "user/keys?per_page=100&page=$page" > "$temporary/keys.json"
        if jq -e --arg key "$public_key" 'any(.[]; (.key | split(" ")[:2] | join(" ")) == $key)' "$temporary/keys.json" >/dev/null; then
            found=true; break
        fi
        [[ "$(jq length "$temporary/keys.json")" == 100 ]] || break
        [[ "$page" != 20 ]] || { echo 'GitHub SSH-key inventory exceeded limit.' >&2; exit 1; }
    done
    if [[ "$found" == false ]]; then
        jq -n --arg title "tjikup@$(hostname)" --arg key "$public_key" '{title:$title,key:$key}' > "$temporary/new-key.json"
        if ! authenticated_api POST user/keys --data-binary "@$temporary/new-key.json" > "$temporary/created.json"; then
            echo 'GitHub key registration failed. GHI_TOKEN needs SSH-key write permission (classic token: admin:public_key).' >&2
            exit 1
        fi
    fi
    unset github_token
fi
for attempt in 1 2 3; do
    if verify_ssh; then
        echo 'GitHub SSH: device key verified for tijptjik.'
        exit 0
    fi
    [[ "$attempt" == 3 ]] || sleep 2
done
echo 'GitHub SSH authentication failed; check the BWS key and GitHub account key permissions.' >&2
exit 1
