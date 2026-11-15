# watch-sign 平台限制分析

> 分析对象：`watch-sign` v0.1.0（本仓库）
> 分析方式：阅读源码 + 依赖源码 + 本机实测（UOS Desktop 20 Professional / aarch64 / 内核 5.10 / glibc 2.28 / rustc 1.99.1）
> 结论一句话：**这套工具本质上是"纯 Rust + usbmuxd"的跨平台程序，不存在 macOS 专有依赖；真正的门槛不在操作系统，而在「USB 通道 + 交互式终端 + 外网 + Apple 账号」。**

---

## 一、结论速览

| 平台 | 构建 | 运行 | 验证状态 | 建议 |
|---|---|---|---|---|
| Linux x86_64 | ✅ | ✅ | 未实测（同源代码，跨架构无差异） | 首选 |
| Linux aarch64（UOS/Deepin/麒麟等 ARM 桌面） | ✅ | ✅ | **本机全流程实测通过** | 首选 |
| macOS（Intel / Apple Silicon） | ✅ 预期可行 | ✅ 预期可行 | 未实测 | 可用，需 Xcode CLT 提供 `cc` |
| Windows 10/11 x64 | ⚠️ 预期可编译 | ⚠️ 依赖 iTunes/Apple 设备服务 | 未实测 | 项目现有结论是改用 iloader fork，不要硬上 |
| WSL2 / 虚拟机 | ✅ | ⚠️ 需 USB 直通 | 未实测 | 可行但折腾，见第五节 |
| Docker | ✅ | ⚠️ 需设备与 socket 直通 | 未实测 | 仅在必须隔离环境时用 |
| 云主机 / CI / Termux / FreeBSD | ❌ | ❌ | — | 不可行，见第五节 |

---

## 二、硬性门槛（缺一项必定失败）

| # | 门槛 | 具体要求 | 依据 |
|---|---|---|---|
| 1 | Rust 工具链 | **≥ 1.85**（`edition = "2024"`），本机 1.99.1 | `Cargo.toml:4` |
| 2 | C 编译器 | `cc`/`gcc`（`ring` 加密库需要编译 C 代码），**不需要 OpenSSL 开发包** | 依赖 `ring` |
| 3 | usbmuxd 通道 | Linux 需安装 `usbmuxd` 服务；macOS 系统自带；Windows 需 iTunes / 「Apple 设备」App | `src/main.rs:276`、`idevice-0.1.65/src/usbmuxd/mod.rs:71-150` |
| 4 | USB 物理连接 | iPhone 必须**用数据线**连接并被信任（不能用 Wi-Fi 配件方式） | `src/main.rs:276-286` |
| 5 | 交互式终端 | 密码、2FA 验证码、证书撤销都要读 `stdin` | `src/main.rs:225`（rpassword）、`236-261`、`288-335` |
| 6 | 外网可达 | anisette 服务器（默认 `https://ani.stikstore.app`，WebSocket）+ Apple 登录/开发者接口 | `vendor/.../remote_v3/mod.rs:26` |
| 7 | 系统时钟准确 | anisette/TLS 依赖正确时间，偏差大会登录失败 | 协议要求 |
| 8 | Apple 账号 | 已开启双重认证；免费账号即可（签名 7 天有效） | 实测事实 |

---

## 三、逐项技术判定（为什么说"平台限制很小"）

### 3.1 没有 macOS 专有依赖 —— 签名是纯 Rust 实现

`apple-codesign`（`vendor/apple-crates/apple-codesign-quick`）用 `cms` / `rsa` / `x509-cert` / `sha2` 自行实现了 CMS 签名与 Mach-O 签名，**不调用 Apple 的 `codesign` 二进制**，也不链接 Security.framework。

同时 isideload 整体走 `rustls` + `ring`，`ldd` 结果只有 libc 级依赖：

```
libgcc_s.so.1  libpthread.so.0  libm.so.6  libdl.so.2  libc.so.6
```

**没有 libssl / libusb / libimobiledevice**。所以你不需要装 `libimobiledevice-dev`、`openssl-dev`，也不需要越狱。

### 3.2 USB 通道靠 usbmuxd，跨平台由 `idevice` 抹平

`idevice` 0.1.65 的地址解析（`usbmuxd/mod.rs`）：

| 平台 | 连接方式 |
|---|---|
| Unix（Linux / macOS） | Unix socket `/var/run/usbmuxd` |
| 非 Unix（Windows） | TCP `127.0.0.1:27015`（由 Apple Mobile Device Service 提供） |
| 任意 | 环境变量 `USBMUXD_SOCKET_ADDRESS` 可覆盖 |

因此在 Windows 上只要能起 Apple 的服务，代码路径是通的（这也是为什么它"理论上能跑"）。

### 3.3 不需要 root / sudo

配对记录是通过 usbmuxd 的 `ReadPairRecord` 消息取得的（`usbmuxd/mod.rs:243`），而不是去读 `/var/lib/lockdown`。本机验证：

```
srw-rw-rw- 1 nobody nogroup 0 11月12 09:27 /var/run/usbmuxd   ← 任何用户可读写
```

**普通用户即可运行整个签名安装流程**，只有"安装 usbmuxd 服务"这一步需要 sudo。

### 3.4 也没有用到 tunneld（不涉及需要 root 的隧道）

Watch 安装走的是 iPhone 的 **companion proxy（经 USB lockdown）**，把手表 62078 端口转发出来再完成配对与安装（`vendor/.../sideload/watch_install.rs:55-100`），不依赖 iOS 17 起的 RemotePairing 隧道服务。

### 3.5 条件编译只有两处，且都已处理

| 位置 | 内容 | 影响 |
|---|---|---|
| `src/main.rs:214-219` | 状态目录 `chmod 0700` 包在 `#[cfg(unix)]` 内 | Windows 上不做权限收紧（仅安全性弱一点，不影响功能） |
| `vendor/.../util/keyring_storage.rs` | 仅 Windows 分支，且本项目 `default-features = false` **未启用 keyring** | 不引入系统钥匙串依赖 |

### 3.6 构建产物的可分发性限制

| 项 | 实测值 | 含义 |
|---|---|---|
| 二进制 | `ELF 64-bit ARM aarch64`，15 MB | 只能给同架构的 Linux 用 |
| glibc 符号上限 | `GLIBC_2.28` | 只能在 glibc ≥ 2.28 的发行版上跑；更老的（如 CentOS 7 / Debian 9）会报 `GLIBC_2.28 not found` |
| 编译器警告 | `unsupported GNU_PROPERTY_TYPE (5) type: 0xc0000000` | 新 rustc + 老 binutils(2.31) 的无害告警；要消掉就装更新的 binutils 或用 `lld` |

要给"比自己系统更老"的机器用时，请让对方**自己编译**，或改用 musl 目标静态链接（`x86_64-unknown-linux-musl`，需注意 ring 的 musl 交叉编译配置）。

**离线可构建**：本机执行 `cargo build --release --offline` 成功（增量为 3.55s），因为被墙的 git 依赖已按 fork 锁定的 commit 以 path 形式内嵌（`Cargo.toml` 的 `[patch]`）。但 `idevice`、`tokio` 等**仍来自 crates.io**，首次全新编译需要能访问 crates.io 或其镜像。

---

## 四、Apple 侧的"人为限制"（与平台无关，但更容易踩）

| 限制 | 说明 | 应对 |
|---|---|---|
| 免费账号签名 7 天 | 到期 App 打不开 | 重跑同一条命令（证书已缓存） |
| 开发证书数量上限 | 通常是 2 张，满了要撤销旧的 | 工具会列出证书让你选撤销 |
| App ID / 设备数量上限 | 免费账号每年有配额 | 固定 bundle ID 长期复用，别每次新建 |
| 只支持 HermesEnabler 限定的 watchOS 版本 | 1.4.1 → watchOS 27.0–27.3 beta1 | 版本不符**不要装** |
| 手表 app 只能装给"已注册"的设备 | 工具会自动把 iPhone 与所有已配对 Watch 注册为开发设备 | 保持 Apple ID 一致 |

---

## 五、运行环境组合的限制

### 5.1 原生 Linux（推荐）
唯一要求：`usbmuxd` 已安装、iPhone 插上并被信任。**本仓库就是在这个环境下跑通的。**

### 5.2 macOS
系统自带 usbmuxd（socket 同为 `/var/run/usbmuxd`），装 Rust（`xcode-select --install` 提供 `cc`）即可。**未实测**，仅有"代码路径一致"的推断；若遇 Apple 侧风控（同一 Apple ID 频繁新建证书）与平台无关。

### 5.3 Windows
- 可编译（需 MSVC 工具链），运行需 iTunes / 「Apple 设备」App 提供的服务（TCP 27015）。
- isideload 在 `[target.'cfg(windows)'.dependencies]` 里把 reqwest 换成 **native-tls**（Schannel），与本项目安装的 rustls provider 不冲突。
- 但**本项目从未在 Windows 实测**，且项目自带教程给出的 Windows 路线是"用已修复的 iloader fork"，说明实测风险不低。**不建议在没有验证能力时硬上 Windows。**

### 5.4 Docker
必须同时满足：
```bash
docker run --rm -it \
  --device=/dev/bus/usb \            # USB 直通
  -v /var/run/usbmuxd:/var/run/usbmuxd \  # 复用宿主机 usbmuxd（推荐，比容器内跑 usbmuxd 省事）
  -v ~/.local/share/watch-sign:/root/.local/share/watch-sign \
  ...
```
`-it` 是**硬要求**，否则密码/2FA 无法输入。

### 5.5 WSL2
WSL 默认看不到 USB，必须用 `usbipd-win` 把 iPhone 直通进发行版，再在 WSL 内装 usbmuxd。
**不要**指望用 `USBMUXD_SOCKET_ADDRESS=<Windows主机IP>:27015` 走 Windows 侧的服务——Apple 的服务只监听本机回环，跨 WSL 网络边界并不可靠。

### 5.6 明确不可行的环境
- **云服务器 / CI Runner**：没有 USB，且无 TTY。
- **Android / Termux**：没有 usbmuxd。
- **FreeBSD**：`idevice` 的 transport 仅覆盖 unix socket 与 TCP，社区无验证，风险自负。

---

## 六、网络与地区限制

| 环节 | 目标 | 本机实测 | 说明 |
|---|---|---|---|
| 构建 | `crates.io` | 走 `rsproxy.cn` 镜像可用 | 首次编译建议配镜像 |
| 构建 | `github.com`（git 协议） | **不可达** | 已通过 `[patch]` + `vendor/` 规避；克隆 xtool 请用 codeload tarball |
| 运行 | `ani.stikstore.app` | 200，1.3s | anisette v3 服务，可用 `--anisette-url` 换源 |
| 运行 | `developerservices2.apple.com` | 200 | App ID / 证书 / 描述文件 |
| 运行 | `gsa.apple.com` | TLS 证书链被替换 → 失败 | **典型诱因：公司/校园网的 SSL 拦截代理**。签名流程走 rustls 严格校验，被拦截会直接登录失败 |

**结论**：可以使用 `https_proxy` 类环境变量（reqwest 启用了 `system-proxy`，anisette 的 WebSocket 也有 proxy 支持），但**不能使用会替换证书的中间人代理**，否则 Apple 登录阶段必挂。

---

## 七、容易误解的点（都不是限制）

| 常见误解 | 事实 |
|---|---|
| "必须用 Mac" | ❌ 不需要，签名是纯 Rust 实现的 |
| "要装 libimobiledevice / ideviceinstaller" | ❌ 不需要，USB 协议栈内置 |
| "要装 OpenSSL 开发包" | ❌ 走 rustls + ring |
| "要越狱 / 要描述文件信任" | ❌ 不需要 |
| "必须 root 运行" | ❌ 普通用户即可（装 usbmuxd 才需要 sudo） |
| "只能用一支 Apple Watch" | ⚠️ 对 `watch-sign` **不成立**（会把所有已配对 Watch 注册为开发设备）；只有备用路径的 xtool `watch_device.py` 要求"恰好一支" |

---

## 八、代码层面的现成坑（与平台无关，但会影响"能不能用"）

| 现象 | 位置 | 说明 |
|---|---|---|
| 同时插多台 iPhone 时行为不定 | `src/main.rs:283-286` | 只取 `devices.first()`，不让你选 |
| `--bundle-id` 改名不彻底 | `src/main.rs:146-178` | 只改主 App 与 `Watch/*.app` 顶层，**不改** Watch app 内嵌的 `.appex` 扩展；带扩展的 IPA 改名后可能不一致。建议长期固定一个 bundle ID，不依赖改名 |
| `--sign-only` 产出的不是 IPA | `src/main.rs:349-356` | 复制出来的是 `.app` 目录，要用 IPA 得自己 `zip` |
| 无 `HOME` 时状态目录落到当前目录 | `src/main.rs:61` | Windows 上 `HOME` 常未设置，建议显式 `--state` |
| 手表安装依赖 iPhone 在线 | `watch_install.rs:55` | 只连手表没用；iPhone 必须连着，Watch 需解锁在附近 |
| 状态目录含私钥 | `src/main.rs:213-219` | `~/.local/share/watch-sign`（0700）不可分享 |

---

## 九、给不同读者的建议

- **只想在一台 Linux 上装一次**：直接照 `TUTORIAL.zh-CN.md` 走，无需关心本文件。
- **要发给别人用**：让对方**自己编译**（避免 glibc/架构不匹配），而不是直接拷贝 `target/release/watch-sign`。
- **非 Linux 桌面**：优先考虑虚拟机里跑 Linux + USB 直通，成本低于在 Windows 上验证编译链。
- **企业/校园网**：先确认没有 SSL 拦截，否则 Apple 登录一定失败。
