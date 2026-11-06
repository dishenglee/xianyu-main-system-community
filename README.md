# 鱼管家社区版

鱼管家是面向闲鱼卖家的本机桌面工具，包含 Rust 业务后端、React 前端和 Tauri 桌面壳。

## 运行边界

- 社区版不包含鱼管家账号登录、许可证激活、设备授权或多租户隔离。
- 后端 API 只绑定 `127.0.0.1`，由桌面 WebView 调用，不是可公开部署的网络服务。
- 闲鱼账号由用户在本机扫码登录；闲鱼 Cookie 保存在本机应用数据目录。
- 业务数据库和凭据都属于本机数据。公开仓库和安装包不包含用户数据。
- 账号数据迁移仅支持本地文件导入导出；社区版不提供绑定鱼管家账户的云端备份。
- 桌面 App 和本地 API 不要求鱼管家账号登录、许可证激活或设备绑定，可以直接启动。
- 社区版不申请或恢复鱼管家产品授权，也不加载签名许可证上下文。遇到 App 通道请求时，会优先通过本地 H5 通道尝试同一业务；如果闲鱼上游只接受 App 签名，该操作在社区版不可用，不会伪造成功。
- 本地功能以及可走 H5/WX 通道的操作不依赖签名服务；实际结果仍受闲鱼 Cookie、上游接口和风控状态影响。签名服务不是所有模块的前置条件，社区版不会绕过签名服务的授权校验，也不提供签名专属能力。

## 开发

需要 Rust stable、Node.js/npm，以及 Tauri 所需的系统依赖。首次运行会自动创建本地 SQLite 数据库。

```sh
npm --prefix frontend-v2 install
npm --prefix frontend-v2 run build
cargo test --manifest-path src-tauri/Cargo.toml -p xianyu-rust-backend-full \
  --test auth_contract --test live_ws_auth --test fixture_parity --test persistence_bootstrap
```

桌面应用测试启动：

```sh
./scripts/start_test_app_prod.sh --restart
```

该脚本构建静态前端和 Tauri 应用后，直接启动单个桌面 App，不启动 Vite 浏览器开发服务器。测试数据目录默认独立于用户生产数据目录。社区版启动不需要许可证信任根或产品账号；依赖外部签名授权的 App 通道默认不可用。

后端独立测试：

```sh
cargo run
```

后端默认仅监听 `127.0.0.1:3000`；设置非回环 `APP_BIND_ADDR` 会启动失败。

## 本地配置

- `APP_DATA_DIR`：应用数据目录，默认 `./data`。
- `APP_BIND_ADDR`：回环地址和端口，默认 `127.0.0.1:3000`。
- `XIANYU_H5_GATEWAY`、`XIANYU_WX_GATEWAY`：闲鱼上游网关覆盖项。
- `RUST_LOG`：日志过滤器。

请勿提交 Cookie、数据库、签名密钥、个人日志或构建产物。提交前运行 `git status --short` 检查工作区。

---

> 👤 作者：涤生AGI | 🐙 github.com/dishenglee | 📡 公众号：涤生AGI
