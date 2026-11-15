# watch-sign

Sign an iOS app with an embedded Apple Watch app and install it on both the iPhone and the Watch — from **Linux or macOS**, with a free Apple ID. No Xcode signing, no jailbreak.

**Verified**: Linux (UOS 20 ARM64) end to end. macOS builds and launches; the sign/install flow has not been tested end to end yet.

Signing is pure Rust — no `codesign`, no Xcode signing, no jailbreak. Devices are reached through usbmuxd, so you do not need libimobiledevice, an OpenSSL dev package, or root.

[中文说明](README.zh-CN.md)

---

## Before you start

| You need | Details |
|---|---|
| Computer | Linux or macOS (Intel / Apple Silicon), with internet access |
| Cable | A USB **data** cable — charge-only cables do not work |
| iPhone | Unlocked, connected over USB, and paired with the Watch |
| Watch | watchOS **27.0 – 27.3 beta 1**, unlocked, on your wrist, near the iPhone |
| Apple ID | Same account as the iPhone, 2FA enabled; a free account is enough |
| Network | `ani.stikstore.app` and Apple domains reachable, **no SSL-inspecting proxy** |

> Windows, WSL, Docker, and CI runners are not recommended. Use Linux or macOS.

---

## 1. Install the tools

### Linux

```bash
sudo apt install usbmuxd build-essential    # Fedora: sudo dnf install usbmuxd gcc make
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### macOS

```bash
xcode-select --install    # provides cc, which ring needs
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

usbmuxd ships with macOS — nothing else to install.

### Check

```bash
rustc --version          # must be >= 1.85
ls -l /var/run/usbmuxd   # should show a socket file
```

Slow crates.io in China? Add a mirror to `~/.cargo/config.toml`:

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

---

## 2. Build

```bash
git clone https://github.com/terencehyz/watch-sign.git
cd watch-sign
cargo build --release
```

The result is `target/release/watch-sign` — one self-contained binary. The first build takes 5–10 minutes and needs no GitHub access (dependencies are vendored). Build on the machine you will run it on: Linux produces ELF, macOS produces Mach-O, and they are not interchangeable. An `unsupported GNU_PROPERTY_TYPE` warning on Linux is harmless.

---

## 3. Pair the Watch (first time only)

This uses xtool's Watch tooling and needs Python 3.12 plus the iPhone on USB. Keep it next to the repository, not inside it:

```bash
cd ..    # leave watch-sign
curl -L -o xtool.tar.gz https://codeload.github.com/oacevedo/xtool/tar.gz/refs/heads/feat/experimental-watchos
tar -xzf xtool.tar.gz
cd xtool-feat-experimental-watchos/tools/watchos

python3.12 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

python watch_device.py --pair --device-info .watchos/device.json
python watch_device.py --reveal-developer-mode
```

1. When the Watch asks **Trust**, tap it.
2. On the Watch: Settings → Privacy & Security → Developer Mode → on → restart → **confirm again after boot**.
3. The iPhone's Watch app must have **exactly one** paired Watch.

Missing Python 3.12?
- Linux: `sudo apt install python3.12 python3.12-venv`; if your distro is too old, install 3.12 with `pyenv`
- macOS: `brew install python@3.12` (or install 3.12 from python.org)

---

## 4. Download the IPA

```bash
curl -L -o ~/Downloads/HermesEnabler.ipa \
  https://github.com/pookjw/HermesEnabler/releases/latest/download/HermesEnabler.ipa
```

This always downloads the newest release. Check its release notes first: it must still support watchOS 27.0 – 27.3 beta 1. The end-to-end test in this repo used 1.4.1; if a newer IPA reports `Expected exactly one .app in Payload`, fall back to 1.4.1.

---

## 5. Sign and install

Keep the iPhone connected and unlocked, and the Watch unlocked nearby. Return to the repository (`cd ../../../watch-sign`) and run:

```bash
./target/release/watch-sign \
  --apple-id you@example.com \
  --ipa ~/Downloads/HermesEnabler.ipa \
  --bundle-id com.yourname.hermesenabler
```

It asks for:

| Prompt | Input |
|---|---|
| Apple ID password | your password (hidden, never stored) |
| 2FA code | the 6 digits; `r` resend, `d` push to devices, `p1` SMS |
| Certificate limit (only if full) | numbers to revoke, e.g. `2` |
| Multiple teams (only if found) | a team number, e.g. `1` |

Then it runs automatically: register devices → create App IDs and the App Group → download profiles → sign → install on iPhone → install on Watch.

**Success**: `Done: companion app installed on iPhone, Watch app installed on the Apple Watch.`

**Two rules**

- Pick `--bundle-id` once and never change it. The tool appends `.<TeamID>` to it, which is expected. Updates must use the same bundle ID and Apple ID, or data is lost.
- It prints `Using device <UDID> (Usb)` first. If it says `(Network)` (common on macOS), the cable is not being used — replug it and rerun.

If only the Watch install fails:

```bash
./target/release/watch-sign ... --sign-only --out ./signed-out
cd ../xtool-feat-experimental-watchos/tools/watchos
python watch_device.py --install /absolute/path/to/watch-sign/signed-out/Watch/*.app
```

### Options

| Option | Meaning |
|---|---|
| `--apple-id` / `--ipa` | required |
| `--bundle-id` | rewrite the bundle ID; keep it fixed |
| `--anisette-url` | anisette server, default `https://ani.stikstore.app` |
| `--state` | certificate/state directory, default `~/.local/share/watch-sign` |
| `--sign-only --out DIR` | sign only; output is an `.app` directory, not an IPA |

---

## After it is installed

1. **First launch**: back up MobileGestalt inside the app *before* enabling the Hermès faces. This is the only rollback.
2. **Every 7 days**: the free-account signature expires and the app stops opening. Rerun the Step 5 command; certificates are cached.
3. **Upgrade**: back up inside the old app, then overwrite with the same bundle ID and Apple ID.
4. **Uninstall**: restore the MobileGestalt backup *inside the app first*, then delete it.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| `Failed to connect to usbmuxd` | Linux: `sudo systemctl restart usbmuxd`. macOS: replug the cable. |
| `No iPhone found over USB` | Use a data cable; unlock the iPhone and tap Trust again. |
| Multiple iPhones connected | The first USB device is used; disconnect the extras. |
| `Using device ... (Network)` | Not on the cable; replug and rerun. |
| TLS/certificate error at login | SSL inspection on the network — change networks, no MITM proxy. |
| anisette unreachable | Use another server: `--anisette-url https://your-server`. |
| 2FA code never arrives | `r` resend, `d` push, `p1` SMS. |
| Certificate limit reached | Revoke an old certificate by number. |
| `IXErrorDomain Code=48` on the Watch | The tool retries 7 times — do not interrupt it. |
| Watch install silent for 3 minutes | Watch asleep or too far; wake it, keep it near the iPhone, rerun. |
| Profile missing the App Group | You are not using the isideload vendored in this repo. |
| `Expected exactly one .app in Payload` | Pass the original `.ipa`, not a folder or extracted bundle. |

---

## Notes

State and certificate keys live in `~/.local/share/watch-sign/` (mode 0700). Never share it. The Apple ID password stays in memory only.

Apple-side limits apply: a free-account signature expires after 7 days, development certificates are capped, and App IDs/devices consume the free account's yearly quota. Reuse one bundle ID instead of creating new ones.
