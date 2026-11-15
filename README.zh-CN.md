# watch-sign

在 **Linux 或 macOS** 上，用一个免费 Apple ID，把带 Apple Watch 组件的 iOS App 同时签名并装进 iPhone 和手表。不需要 Xcode 签名，也不需要越狱。

**验证状态**：Linux（UOS 20 ARM64）全流程跑通；macOS 构建与启动通过，签名安装尚未端到端实测。

签名是纯 Rust 实现——不调用 `codesign`，不需要 Xcode 签名，也不需要越狱。设备通过 usbmuxd 访问，所以不需要 libimobiledevice、OpenSSL 开发包或 root。

[English](README.md)

---

## 开始前

| 需要 | 说明 |
|---|---|
| 电脑 | Linux 或 macOS（Intel / Apple Silicon），能上网 |
| 数据线 | 必须是 **USB 数据线**，充电线不行 |
| iPhone | 解锁、用 USB 连接、已和手表配对 |
| 手表 | watchOS **27.0 – 27.3 beta 1**，解锁、戴在手上、靠近 iPhone |
| Apple ID | 与 iPhone 同一个，已开双重认证；免费账号即可 |
| 网络 | 能访问 `ani.stikstore.app` 和 Apple 域名，**没有 SSL 拦截代理** |

> Windows、WSL、Docker、CI 不推荐，直接用 Linux 或 macOS。

---

## 1. 安装环境

### Linux

```bash
sudo apt install usbmuxd build-essential    # Fedora：sudo dnf install usbmuxd gcc make
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### macOS

```bash
xcode-select --install    # 提供 cc，ring 编译 C 代码时要用
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

macOS 自带 usbmuxd，不需要额外安装。

### 检查

```bash
rustc --version          # 必须 >= 1.85
ls -l /var/run/usbmuxd   # 能看到 socket 文件即可
```

国内下载 crates.io 慢的话，把镜像写进 `~/.cargo/config.toml`：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

---

## 2. 编译

```bash
git clone https://github.com/terencehyz/watch-sign.git
cd watch-sign
cargo build --release
```

产物是 `target/release/watch-sign`，单文件直接运行。首次编译 5–10 分钟；依赖已内嵌在 `vendor/`，编译时不需要访问 GitHub。请在哪台机器运行就在哪台编译：Linux 产出 ELF，macOS 产出 Mach-O，不能互换。Linux 上出现 `unsupported GNU_PROPERTY_TYPE` 是无害警告。

---

## 3. 配对手表（只在第一次做）

这一步用 xtool 的手表工具，需要 Python 3.12，并且 iPhone 要连着 USB。把 xtool 解压在仓库旁边，不要放进仓库：

```bash
cd ..    # 离开 watch-sign
curl -L -o xtool.tar.gz https://codeload.github.com/oacevedo/xtool/tar.gz/refs/heads/feat/experimental-watchos
tar -xzf xtool.tar.gz
cd xtool-feat-experimental-watchos/tools/watchos

python3.12 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

python watch_device.py --pair --device-info .watchos/device.json
python watch_device.py --reveal-developer-mode
```

1. 手表弹出「信任」时点信任。
2. 到手表上：设置 → 隐私与安全性 → 开发者模式 → 打开 → 重启 → **开机后再确认一次**。
3. iPhone 的 Watch App 里只能有**一支**配对中的手表。

没有 Python 3.12？
- Linux：`sudo apt install python3.12 python3.12-venv`；发行版太老就用 `pyenv` 装 3.12
- macOS：`brew install python@3.12`（或从 python.org 安装 3.12）

---

## 4. 下载 IPA

```bash
curl -L -o ~/Downloads/HermesEnabler.ipa \
  https://github.com/pookjw/HermesEnabler/releases/latest/download/HermesEnabler.ipa
```

这样每次都会下载最新版。下载后先看一眼 release 说明：仍需支持 watchOS 27.0 – 27.3 beta 1。本仓库的端到端验证用的是 1.4.1；如果最新 IPA 报 `Expected exactly one .app in Payload`，就退回 1.4.1。

---

## 5. 签名并安装

保持 iPhone 连着 USB 且解锁、手表解锁并放在附近。回到仓库目录（`cd ../../../watch-sign`），然后执行：

```bash
./target/release/watch-sign \
  --apple-id 你的ID@example.com \
  --ipa ~/Downloads/HermesEnabler.ipa \
  --bundle-id com.yourname.hermesenabler
```

它会依次询问：

| 提示 | 输入 |
|---|---|
| Apple ID 密码 | 密码（无回显，不落盘） |
| 2FA 验证码 | 6 位码；`r` 重发、`d` 推设备、`p1` 走短信 |
| 证书已满（仅当出现） | 要撤销的编号，如 `2` |
| 多个开发团队（仅当出现） | 团队编号，如 `1` |

之后全自动：注册设备 → 申请 App ID 与 App Group → 下载描述文件 → 本地签名 → 装 iPhone → 装手表。

**成功标志**：`Done: companion app installed on iPhone, Watch app installed on the Apple Watch.`

**两条规则**

- `--bundle-id` 定了就别改。工具会自动给它追加 `.<团队ID>`，这是正常的。更新必须同 bundle ID + 同 Apple ID，否则等于重装丢数据。
- 启动后会先打印 `Using device <UDID> (Usb)`。如果打印 `(Network)`（macOS 常见），说明没走数据线，重新插线再跑。

只有最后装手表失败时：

```bash
./target/release/watch-sign ... --sign-only --out ./signed-out
cd ../xtool-feat-experimental-watchos/tools/watchos
python watch_device.py --install /绝对路径/watch-sign/signed-out/Watch/*.app
```

### 参数

| 参数 | 说明 |
|---|---|
| `--apple-id` / `--ipa` | 必填 |
| `--bundle-id` | 改写 bundle ID；建议固定 |
| `--anisette-url` | anisette 服务，默认 `https://ani.stikstore.app` |
| `--state` | 证书与状态目录，默认 `~/.local/share/watch-sign` |
| `--sign-only --out DIR` | 只签名；输出 `.app` 目录，不是 IPA |

---

## 装完之后

1. **第一次打开**：先在 App 里备份 MobileGestalt，再启用 Hermès 表盘。这是唯一的回滚机会。
2. **每 7 天**：免费账号签名到期，App 打不开是正常现象。原样重跑第 5 步命令，证书有缓存。
3. **升级**：先在旧版里备份，再用同 bundle ID、同 Apple ID 覆盖安装。
4. **卸载**：先在 App 内还原 MobileGestalt 备份，再删除，否则系统设置会一直保持改过的状态。

---

## 出错速查

| 看到什么 | 怎么办 |
|---|---|
| `Failed to connect to usbmuxd` | Linux：`sudo systemctl restart usbmuxd`；macOS：重新插拔数据线 |
| `No iPhone found over USB` | 换数据线；解锁 iPhone 后重新点「信任」 |
| 同时插了多台 iPhone | 只会用第一台 USB 设备；拔掉多余的 |
| `Using device ... (Network)` | 没走数据线，重新插线再跑 |
| 登录时报证书 / TLS 错误 | 网络有 SSL 拦截，换网络，别用中间人代理 |
| anisette 连不上 | 换源：`--anisette-url https://你的服务` |
| 2FA 一直收不到 | `r` 重发、`d` 推设备、`p1` 转短信 |
| 证书数量超限 | 按列表输入编号，撤销旧证书 |
| 手表报 `IXErrorDomain Code=48` | 工具会自动重试 7 次，**别中断** |
| 手表安装 3 分钟没动静 | 手表睡了或太远；唤醒、贴近 iPhone 后重跑 |
| 提示描述文件缺 App Group | 用的不是本仓库 `vendor/` 里的 isideload |
| `Expected exactly one .app in Payload` | 传了文件夹或解压后的包，要用原始 `.ipa` |

---

## 说明

证书与状态在 `~/.local/share/watch-sign/`（权限 0700），**不要分享**。Apple ID 密码只在内存里。

Apple 侧还有配额限制：免费账号签名 7 天到期，开发证书有数量上限，App ID / 设备会占用免费账号的年度配额。建议长期复用同一个 bundle ID，不要每次新建。
