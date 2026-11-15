# 在 Apple Watch 上装 HermesEnabler

一根数据线 + 一个免费 Apple ID，在 Linux 上签名并同时装进 iPhone 和已配对的手表。
已在 UOS 20（ARM64）+ iOS 27.3 + watchOS 27.2 跑通。

**开始前先确认这四条**，任何一条不满足就别往下做：

- 手表是 **watchOS 27.0 – 27.3 beta 1**（HermesEnabler 1.4.1 只支持这个区间）
- Apple ID 与 iPhone 同一个，已开双重认证，免费账号即可
- 能访问 `ani.stikstore.app` 和 Apple 域名，**没有 SSL 拦截代理**
- 在一个**能交互的终端**里操作（要输密码和验证码）

> 平台可选范围（macOS / Windows / 虚拟机）见 [PLATFORM.md](./PLATFORM.md)。

---

## 第 1 步 · 打通 USB

**做什么**：让电脑认得 iPhone。

```bash
sudo apt install usbmuxd          # Fedora 系用 sudo dnf install usbmuxd
ls -l /var/run/usbmuxd            # 能列出 socket 文件就算通
```

然后：iPhone **解锁** → 插**数据线** → 手机弹「信任此电脑」→ 点信任 → 输锁屏密码。

**应该看到**：`srw-rw-rw- ... /var/run/usbmuxd`

**没成**：文件不存在就先 `sudo systemctl restart usbmuxd`；充电线不行，必须数据线。

---

## 第 2 步 · 配对手表

**做什么**：让 iPhone 允许向手表推 App，手表允许装开发版。**只在第一次做。**

先用 xtool 的 watch 工具（`git clone` 常被墙，用 tarball）：

```bash
curl -L -o xtool.tar.gz https://codeload.github.com/oacevedo/xtool/tar.gz/refs/heads/feat/experimental-watchos
tar -xzf xtool.tar.gz && cd xtool-feat-experimental-watchos/tools/watchos
python3.12 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
```

再跑两条命令：

```bash
python watch_device.py --pair --device-info .watchos/device.json
python watch_device.py --reveal-developer-mode
```

第一条发出后**手表会弹「信任」**，点信任。
第二条之后到手表上操作：设置 → 隐私与安全性 → 开发者模式 → 打开 → 重启 → **开机后再确认一次**。

**没成**：手机 Watch App 里只能有**一支**配对中的手表，否则报 `Expected exactly one paired Watch`；开发者模式开关不出现，就在手表解锁状态下重跑第二条命令。

---

## 第 3 步 · 编译签名工具

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustc --version        # 要 ≥ 1.85

cd watch-sign
cargo build --release  # 首次 5–10 分钟
```

**crates.io 慢就加镜像**，写入 `~/.cargo/config.toml`：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

**应该看到**：`Finished \`release\` profile`，产物是 `target/release/watch-sign`（单文件，直接运行）。

**注意**：构建时出现 `unsupported GNU_PROPERTY_TYPE` 是无害警告，忽略。本项目**不需要访问 GitHub**，被墙的依赖已内嵌在 `vendor/`。

---

## 第 4 步 · 签名并安装

**先下 IPA**（1.4.1，本文验证过的版本）：

```
https://github.com/pookjw/HermesEnabler/releases/download/1.4.1/HermesEnabler.ipa
SHA256 5b7894cf38ad7f4cbe08743e4fed41db8b3f06b133c90db8aa02f43ef1e66092
```

**保持三个状态**：iPhone 连着 USB 且解锁 · 手表戴着解锁在附近 · 终端在前台等你输入。

```bash
./target/release/watch-sign \
  --apple-id 你的ID@example.com \
  --ipa ~/Downloads/HermesEnabler.ipa \
  --bundle-id com.han.hermesenabler
```

**它会依次问四件事**：

| 提示 | 输入 |
|---|---|
| Apple ID 密码 | 密码（无回显，不落盘） |
| 2FA 验证码 | 6 位码；没收到输 `r` 重发、`d` 推设备、`p1` 走短信 |
| 开发证书已满（仅当出现） | 要撤销的编号，如 `2` |
| 多个开发团队（仅当出现） | 团队编号，如 `1` |

**之后全自动**，进度依次是：注册设备 → 申请 App ID 与 App Group → 下载描述文件 → 本地签名 → 装 iPhone → 装手表。

**成功标志**：终端打印 `Done: companion app installed on iPhone, Watch app installed on the Apple Watch.`，手表桌面出现图标且能点开。

**两条务必记住**：

- `--bundle-id` 定了就**别改**（工具会自动加 `.<团队ID>`，正常）。更新 App 必须同 bundle ID + 同 Apple ID，否则等于重装丢数据。
- 如果只卡在最后装手表：加 `--sign-only --out ./signed-out` 只签名，再用第 2 步的环境执行 `python watch_device.py --install ./signed-out/Watch/HermesEnabler2.app`。

---

## 装完之后

1. **第一次打开**：在手表上先「备份 MobileGestalt」，再启用 Hermès 表盘。这是唯一的回滚机会。
2. **每 7 天**：签名到期 App 打不开是正常现象，原样重跑第 4 步那条命令即可，证书有缓存不会重注册。
3. **升级**：先在旧版里备份，再同 bundle ID / 同 Apple ID 覆盖安装。
4. **卸载**：必须在 App 内**先还原 MobileGestalt 备份**再删除，否则系统设置会一直是改过的状态。

---

## 出错速查

| 看到什么 | 怎么办 |
|---|---|
| `Failed to connect to usbmuxd` | 第 1 步没做完，检查 usbmuxd 服务与 socket 文件 |
| `No iPhone found over USB` | 换数据线；解锁后重新「信任此电脑」 |
| 登录阶段报证书/TLS 错误 | 网络有 SSL 拦截，换网络，别用中间人代理 |
| anisette 连不上 | 换源：`--anisette-url https://你的服务` |
| 2FA 一直收不到 | 输 `r` 重发、`d` 推设备、`p1` 转短信 |
| 证书数量超限 | 按列表输编号撤销旧的（旧电脑那张通常可以撤） |
| 手表端报 `IXErrorDomain Code=48` | 工具自带 7 次重试，**别中断**，等它跑完 |
| 手表安装 3 分钟没动静 | 手表睡了或太远，解锁戴手上贴近 iPhone 重跑 |
| 提示描述文件缺 App Group | 用的不是本仓库 `vendor/` 里的 isideload |
| `Expected exactly one .app in Payload` | 传了文件夹或已解压的包，要用原始 `.ipa` |

---

## 附：参数与文件

| 参数 | 说明 |
|---|---|
| `--apple-id` / `--ipa` | 必填 |
| `--bundle-id` | 改写 bundle ID，建议固定 |
| `--anisette-url` | 默认 `https://ani.stikstore.app` |
| `--state` | 证书与状态目录，默认 `~/.local/share/watch-sign` |
| `--sign-only --out DIR` | 只签名，输出 `.app` 目录（不是 IPA） |

**私密文件，不能分享**：`~/.local/share/watch-sign/`（开发证书私钥）、手表工具目录下的 `.watchos/`（配对私钥）。
Apple ID 密码只在内存里，任何工具都不该存。
