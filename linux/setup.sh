#!/bin/bash
set -Eeuo pipefail
umask 077

trap 'printf "DigiSafe setup failed at line %s. Fix the error above and rerun setup.\n" "$LINENO" >&2' ERR

# Resolve assets and Cargo.toml relative to this script, regardless of the caller's cwd.
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

if (( EUID == 0 )); then
    printf "Run setup as your normal user; sudo is used only to install packages.\n" >&2
    exit 1
fi

usage() {
    printf 'Usage: %s [--reset-backblaze|--restore-pepper]\n' "$0"
}

provision_backblaze() {
    printf "Provisioning Backblaze API Bundle...\n"
    {
        systemd-ask-password -n "Enter API Key Name:"
        printf "\0"
        systemd-ask-password -n "Enter API Key ID:"
        printf "\0"
        systemd-ask-password -n "Enter App Secret Key:"
    } | systemd-creds encrypt --user --name=backblaze - "$temp_dir/backblaze.cred"
    chmod 600 -- "$temp_dir/backblaze.cred"
    mv -- "$temp_dir/backblaze.cred" "$config_dir/backblaze.cred"
}

provision_pepper() {
    printf 'Deriving pepper from your original seed (requires 4 GiB of memory)...\n'
    systemd-ask-password -n "Enter original pepper seed:" | \
        argon2 digipepper -id -t 4 -m 22 -p 1 -l 32 -r -v 13 | \
        systemd-creds encrypt --user --name=digipepper - "$temp_dir/digipepper.cred"
    local pepper
    pepper=$(systemd-creds decrypt --user --name=digipepper "$temp_dir/digipepper.cred" -)
    if [[ ! "$pepper" =~ ^[[:xdigit:]]{64}$ ]]; then
        unset pepper
        printf 'Generated pepper is invalid; existing credential was preserved.\n' >&2
        exit 1
    fi
    unset pepper
    chmod 600 -- "$temp_dir/digipepper.cred"
    if [[ -e "$config_dir/digipepper.cred" ]]; then
        local backup
        backup=$(mktemp "$config_dir/digipepper.cred.backup.XXXXXXXX")
        cp -- "$config_dir/digipepper.cred" "$backup"
        printf 'Previous encrypted pepper saved to %s\n' "$backup"
    fi
    mv -- "$temp_dir/digipepper.cred" "$config_dir/digipepper.cred"
}

case "${1:-}" in
    --reset-backblaze|--restore-pepper)
        [[ $# == 1 ]] || { usage >&2; exit 2; }
        config_dir="$HOME/.config/digisafe"
        mkdir -p -- "$config_dir"
        chmod 700 -- "$config_dir"
        temp_dir=$(mktemp -d "$config_dir/.setup.XXXXXXXX")
        trap 'rm -rf -- "$temp_dir"' EXIT
        if [[ "$1" == --restore-pepper ]]; then
            for tool in argon2 systemd-creds systemd-ask-password; do
                command -v "$tool" >/dev/null
            done
            provision_pepper
            printf 'Pepper credential restored. Try opening your existing vault.\n'
        else
            provision_backblaze
            printf 'Backblaze credentials updated. Pepper unchanged.\n'
        fi
        exit 0
        ;;
    '') ;;
    *) usage >&2; exit 2 ;;
esac

printf "Checking DigiSafe dependencies...\n"
for tool in cargo rustc sudo apt-get dpkg-query; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf "Missing %s. Setup requires Debian/Ubuntu, sudo, and a working Rust toolchain.\n" "$tool" >&2
        exit 1
    fi
done
cargo --version
rustc --version

packages=(build-essential pkg-config dpkg-dev argon2 systemd libcap2-bin libwayland-dev libxkbcommon-dev)
missing=()
for package in "${packages[@]}"; do
    if [[ $(dpkg-query -W -f='${Status}' "$package" 2>/dev/null || true) != 'install ok installed' ]]; then
        missing+=("$package")
    fi
done
if (( ${#missing[@]} )); then
    sudo apt-get update
    sudo apt-get install -y "${missing[@]}"
fi
if ! cargo deb --version >/dev/null 2>&1; then
    cargo install cargo-deb --locked
fi
for tool in argon2 systemd-creds systemd-ask-password; do
    command -v "$tool" >/dev/null
done
pkg-config --exists wayland-client xkbcommon

config_dir="$HOME/.config/digisafe"
mkdir -p -- "$config_dir"
chmod 700 -- "$config_dir"
temp_dir=$(mktemp -d "$config_dir/.setup.XXXXXXXX")
trap 'rm -rf -- "$temp_dir"' EXIT

# Exercise user-scoped encryption before requesting any real secrets.
printf 'digisafe-setup-check' | systemd-creds encrypt --user --name=setup-check - "$temp_dir/check.cred"
systemd-creds decrypt --user --name=setup-check "$temp_dir/check.cred" /dev/null

# Existing credentials must be usable; never silently replace a vault's pepper.
for name in digipepper backblaze; do
    credential="$config_dir/$name.cred"
    if [[ -e "$credential" ]]; then
        if [[ ! -s "$credential" ]]; then
            printf "Existing credential is empty: %s. Restore or repair it before rerunning.\n" "$credential" >&2
            exit 1
        fi
        chmod 600 -- "$credential"
        systemd-creds decrypt --user --name="$name" "$credential" /dev/null
        if [[ "$name" == digipepper ]]; then
            pepper=$(systemd-creds decrypt --user --name=digipepper "$credential" -)
            if [[ ! "$pepper" =~ ^[[:xdigit:]]{64}$ ]]; then
                unset pepper
                printf 'Saved pepper is invalid: expected 64 hex characters. Restore the original pepper or recreate it using the original seed before opening an existing vault.\n' >&2
                exit 1
            fi
            unset pepper
        fi
    fi
done

printf "Building and installing DigiSafe before requesting credentials...\n"
cargo deb --install

if [[ ! -e "$config_dir/digipepper.cred" ]]; then
    provision_pepper
else
    printf "Reusing existing pepper credential.\n"
fi

if [[ ! -e "$config_dir/backblaze.cred" ]]; then
    provision_backblaze
else
    printf "Reusing existing Backblaze credentials.\n"
fi

printf "DigiSafe setup finished.\n"
