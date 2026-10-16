## DigiSafe

A digital safe for secure storage of everything LastPass leaks and KeePass fails to backup.

### Features

 - 4GiB Argon2id / Sha3 salted and peppered master password derivation.
 - TPM locked pepper via systemd-creds.
 - Secret memory based master password storage.
 - The entire app is unswappable and undumpable.
 - Dynamic linker hijacking blockage.
 - Tracing blockage. (ptrace, strace, ltrace, gdb)
 - Forensic-resistant clipboard clearing mechanism.
 - Backblaze sync on load/save, with api creds TPM locked via systemd-creds.
 - Atomic file io.
 - Reed-Solomon 8:4 erasure coding with BLAKE3 checksums and 4KiB+ shards.
 - XChaCha20Poly1305 encryption.
 - LZ4 level 9 compression.
 - Simple TLV serialization.
 - CPU rending only via tiny-skia.
 - Wayland only.

### Linux setup

On Debian/Ubuntu, install a Rust toolchain and run `./linux/setup.sh` as your
normal user. Setup uses sudo to install missing system dependencies
(`build-essential`, `pkg-config`, `dpkg-dev`, `argon2`, `systemd`, `libcap2-bin`,
`libwayland-dev`, and `libxkbcommon-dev`) and installs `cargo-deb` if needed.
It checks credential encryption and builds/installs DigiSafe before asking
for the pepper seed and Backblaze credentials.

Package installation grants `/usr/bin/digisafe` the `CAP_IPC_LOCK` capability
so it can lock its memory, including the 4 GiB password derivation, without
being constrained by the desktop session's memlock limit. This is reapplied
on upgrades. Launch the installed executable; a build in `target/` does not
have this capability.

Rerunning setup reuses the encrypted credentials in `~/.config/digisafe`.
Failed provisioning does not save a partial credential. Existing empty or
undecryptable credentials stop setup for repair; they are never automatically
replaced. Keep the original pepper credential when using an existing vault.

To re-enter only the Backblaze credentials, run
`./linux/setup.sh --reset-backblaze`. This skips the build and preserves the
pepper. The old Backblaze file is replaced only after encryption succeeds.

To recover an invalid pepper credential, run `./linux/setup.sh --restore-pepper`
and enter the original pepper seed used for the vault. This skips rebuilding,
backs up the previous encrypted pepper, and validates the regenerated value
before replacing it. It preserves Backblaze credentials. A different seed
will not unlock an existing vault.

### Known Weaknesses

 - iced String buffers.
 - no virtual keyboard input.
 - bpftrace.
 - wayland-data-control.

#### Screenshots:
![ScreenShot](https://github.com/JoelBondurant/digisafe/blob/main/doc/img/unlock_screen.png)

![ScreenShot](https://github.com/JoelBondurant/digisafe/blob/main/doc/img/empty_entry.png)

![ScreenShot](https://github.com/JoelBondurant/digisafe/blob/main/doc/img/sample_entry.png)

