# Installing HermesEnabler on an Apple Watch

One USB cable and one free Apple ID: sign the app on Linux and install it on both the iPhone and its paired Watch.
Verified on UOS 20 (ARM64) + iOS 27.3 + watchOS 27.2.

**Check these four things first.** If any one fails, stop here:

- Watch runs **watchOS 27.0 – 27.3 beta 1** (the only range HermesEnabler 1.4.1 supports)
- The Apple ID matches the iPhone's, has 2FA enabled, and a free account is enough
- `ani.stikstore.app` and Apple's domains are reachable, with **no SSL-inspecting proxy**
- You are in an **interactive terminal** (a password and a 2FA code get typed)

> Other platforms (macOS / Windows / VMs) are covered in [PLATFORM.md](./PLATFORM.md).

---

## Step 1 · Open the USB path

**What**: make the computer see the iPhone.

```bash
sudo apt install usbmuxd          # Fedora: sudo dnf install usbmuxd
ls -l /var/run/usbmuxd            # a socket file here means the path is open
```

Then: **unlock** the iPhone → plug in a **data cable** → tap **Trust** → enter the passcode.

**Expect**: `srw-rw-rw- ... /var/run/usbmuxd`

**If not**: no such file → `sudo systemctl restart usbmuxd`. Charge-only cables do not work.

---

## Step 2 · Pair the Watch

**What**: let the iPhone push apps to the Watch, and let the Watch accept development builds. **First time only.**

Start with xtool's Watch tooling (`git clone` is often blocked, so use the tarball):

```bash
curl -L -o xtool.tar.gz https://codeload.github.com/oacevedo/xtool/tar.gz/refs/heads/feat/experimental-watchos
tar -xzf xtool.tar.gz && cd xtool-feat-experimental-watchos/tools/watchos
python3.12 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
```

Then two commands:

```bash
python watch_device.py --pair --device-info .watchos/device.json
python watch_device.py --reveal-developer-mode
```

The first one makes the Watch pop a **Trust** prompt — accept it.
After the second, on the Watch: Settings → Privacy & Security → Developer Mode → on → restart → **confirm again after boot**.

**If not**: the iPhone's Watch app must have **exactly one** paired Watch, otherwise you get `Expected exactly one paired Watch`. If the Developer Mode switch never appears, re-run the second command with the Watch unlocked.

---

## Step 3 · Build the signing tool

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustc --version        # must be >= 1.85

cd watch-sign
cargo build --release  # 5–10 minutes the first time
```

**Slow crates.io?** Add a mirror in `~/.cargo/config.toml`:

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

**Expect**: `Finished \`release\` profile`, with the artifact at `target/release/watch-sign` (one file, run it directly).

**Note**: `unsupported GNU_PROPERTY_TYPE` during the build is a harmless warning. This project needs **no GitHub access** — the blocked dependency is vendored under `vendor/`.

---

## Step 4 · Sign and install

**Download the IPA first** (1.4.1, the version verified here):

```
https://github.com/pookjw/HermesEnabler/releases/download/1.4.1/HermesEnabler.ipa
SHA256 5b7894cf38ad7f4cbe08743e4fed41db8b3f06b133c90db8aa02f43ef1e66092
```

**Hold three states**: iPhone on USB and unlocked · Watch on your wrist, unlocked, nearby · terminal in the foreground waiting for input.

```bash
./target/release/watch-sign \
  --apple-id you@example.com \
  --ipa ~/Downloads/HermesEnabler.ipa \
  --bundle-id com.han.hermesenabler
```

**It asks four things**:

| Prompt | Type |
|---|---|
| Apple ID password | the password (no echo, never stored) |
| 2FA code | the 6 digits; nothing arrived → `r` resend, `d` push, `p1` SMS |
| Development certificate limit (only if hit) | numbers to revoke, e.g. `2` |
| Multiple teams (only if found) | a team number, e.g. `1` |

**Then it runs itself**: register devices → request App IDs and the App Group → download profiles → sign locally → install on iPhone → install on Watch.

**Success looks like**: `Done: companion app installed on iPhone, Watch app installed on the Apple Watch.` and an app icon on the Watch that actually opens.

**Two rules**:

- Pick `--bundle-id` once and **never change it** (the tool appends `.<TeamID>` — expected). Updates must reuse the same bundle ID and Apple ID, or it becomes a fresh install and data is lost.
- If only the final Watch install fails: add `--sign-only --out ./signed-out`, then use the Step 2 environment to run `python watch_device.py --install ./signed-out/Watch/HermesEnabler2.app`.

---

## After it is installed

1. **First launch**: back up MobileGestalt on the Watch *before* enabling the Hermès faces. It is your only rollback.
2. **Every 7 days**: the signature expires and the app stops opening — that is normal. Run the Step 4 command again; certificates are cached, nothing is re-registered.
3. **Upgrade**: back up inside the old app, then overwrite with the same bundle ID and Apple ID.
4. **Uninstall**: restore the MobileGestalt backup **inside the app first**, then delete it, or system settings stay modified.

---

## When it goes wrong

| You see | Do this |
|---|---|
| `Failed to connect to usbmuxd` | Step 1 is incomplete — check the usbmuxd service and socket |
| `No iPhone found over USB` | Change the cable; unlock and re-trust |
| Certificate/TLS error at login | SSL inspection on the network — change networks, no MITM proxy |
| Cannot reach anisette | Point elsewhere: `--anisette-url https://your-server` |
| 2FA code never arrives | `r` to resend, `d` to push, `p1` for SMS |
| Certificate limit reached | Revoke an old one by number (the old computer's usually qualifies) |
| `IXErrorDomain Code=48` on the Watch | The tool retries 7 times — **do not interrupt it** |
| Watch install silent for 3 minutes | Watch asleep or too far — unlock it, keep it near the iPhone, rerun |
| Profile is missing the App Group | You are not using the isideload from this repo's `vendor/` |
| `Expected exactly one .app in Payload` | You passed a folder or an extracted bundle — use the original `.ipa` |

---

## Appendix · options and files

| Option | Meaning |
|---|---|
| `--apple-id` / `--ipa` | required |
| `--bundle-id` | rewrite the bundle ID; keep it fixed |
| `--anisette-url` | default `https://ani.stikstore.app` |
| `--state` | certificate/state directory, default `~/.local/share/watch-sign` |
| `--sign-only --out DIR` | sign only; output is an `.app` directory, not an IPA |

**Private, never share**: `~/.local/share/watch-sign/` (development certificate private keys) and `.watchos/` under the Watch tools (pairing keys).
The Apple ID password lives in memory only; no tool should store it.
