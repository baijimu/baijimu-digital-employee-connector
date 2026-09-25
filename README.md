# Codex 数字员工连接器 1.0.0

应用 ID 为 `digital-employee-connector`。独立托管 Codex app-server，通过私有 stdio 处理任务、轮次、审批和通知，不连接桌面 IPC、桌面 control socket 或共享 daemon。桌面退出不影响此运行时；Connector 停止时其 stdio 关闭，后台子进程随之结束。

## 数据与认证

Bridge Agent 分配的 `BAIJIMU_LOCAL_APP_DATA_DIR/runtime/codex` 是唯一 `CODEX_HOME`；未由宿主分配时使用用户目录下 `.baijimu-digital-employee-connector/runtime/codex`。桌面 `.codex` 不作为后备目录。运行目录若通过符号链接落入桌面目录会被拒绝。平台项目默认检出到本应用数据目录下的 `projects`。

连接器从 `DIGITAL_EMPLOYEE_CODEX_BINARY` 读取运维明确配置的 Codex CLI 路径，未设置则执行 PATH 中的 `codex`。安装环境必须预先提供兼容 CLI（版本要求由代码中的协议下限声明）；此 Connector 不安装或更新用户桌面 CLI。

子进程不继承桌面的 `CODEX_HOME`、`OPENAI_API_KEY`、`CODEX_API_KEY` 和 Bridge 令牌环境变量，强制使用独立目录的文件式凭据存储，不复用系统钥匙串登录态。数字员工需要单独授权：经已授权的 `request` 方法调用 app-server `account/login/start`，并按其返回的登录流程完成认证；用 `account/read` 核实。本机 CLI 也可在明确指定本应用 `CODEX_HOME` 和 `cli_auth_credentials_store="file"` 后登录。

这是会话、配置和凭据隔离，**不是操作系统用户或文件系统沙箱隔离**。需要多租户安全隔离时，应在不同 OS 用户或容器中分别部署 Connector，并按员工权限配置 app-server 沙箱与审批。

## 接口

`connector.json` 是方法、权限、事件和默认端口的唯一清单。HTTP 只绑定 loopback，需要宿主 Connector token 和可信工作区 Header。保留 `startThread`、`resumeThread`、`startTurn`、`steerTurn`、`interruptTurn`、`pendingRequests`、`respondToRequest`、`request`、任务查询与项目准备方法。

默认端口来自清单，与桌面 Connector 独立；可用 `--port` 或 `DIGITAL_EMPLOYEE_CONNECTOR_PORT` 配置。桌面任务 ID 不代表本运行时的任务，禁止把此 Connector 用作桌面失联后的透明替代。

## 验证与发布

源码从桌面 Connector 2.2.0 的 RPC 实现拆出（上游提交 `796ee6a691ea2bb8684493bfa0a37fd0afb54ab8`，保留 MIT 许可），删除共享 daemon、桌面安装器和旧 JavaScript 运行时。

运行 `cargo fmt --check`、`cargo test --locked`、`npm test` 和 Python 发布合同测试。独立发布入口为本仓库 `.github/workflows/release.yml`，签名、OSS 和来源发布凭据必须在本仓库独立配置。不得借用桌面 Connector 的流水线或应用身份。来源版本冻结后提交独立市场审核，待审不等于公开可安装。

发布仓库需配置 Secrets：`APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`SSL_COM_USERNAME`、`SSL_COM_PASSWORD`、`SSL_COM_CREDENTIAL_ID`、`SSL_COM_TOTP_SECRET`、`OSS_ACCESS_KEY_ID`、`OSS_ACCESS_KEY_SECRET`、`LOCAL_APP_MARKET_PUBLISH_TOKEN`。Variables：`APPLE_SIGNING_IDENTITY`、`LOCAL_APP_OWNER_WORKSPACE_ID`、`OSS_BUCKET`、`OSS_ENDPOINT`、`OSS_REGION`、`OSS_PUBLIC_BASE`。值由签名、存储和来源应用各自所有者提供。

先对 `main` 运行 `publish=false` 三平台演练，通过后从已验证主线提交创建 `v1.0.0`，再对该标签运行 `publish=true`。不可变制品发布中断时保留第一次成功的签名字节；不能用重新签名产生的不同字节覆盖同一版本。市场待审需独立审核，禁止作者自行批准。

首次发布版本也必须先获得当前用户对准确版本的确认。PR 和主线提交自动执行三平台源码验证，不生成发布制品。正式流水线在构建前检查签名、存储及来源发布配置是否齐全；配置存在不代表凭据有效，仍以签名与发布结果为准。Windows 成品必须通过 Authenticode 验签并带可信时间戳。

可先设置 `configuration_only=true`、`publish=false`，单独核对运行器实际可访问的发布配置，不构建制品、不冻结版本。此检查只报告缺失项名称。

macOS 签名会将临时钥匙串注册到用户搜索列表，完成或失败后恢复原列表并删除临时凭据。可通过 `verify.yml` 的 `verify_macos_signing=true` 在正式发布前验证真实签名、验签与清理，不生成应用发布制品。

### 中断恢复

如果三平台打包已完成，而 OSS、GitHub 或来源版本发布中断，重新运行同一发布工作流，保持 `release_ref` 和 `publish=true`，将 `recovery_run_id` 设置为**最初产出签名制品的运行 ID**。恢复跳过构建和签名，验证来源仓库、工作流、提交、标签、平台、摘要和发布模式，再继续不可变上传与来源发布。不得直接点击失败运行的全部任务重跑来重新签名。

流水线保留原始制品 90 天；恢复必须三平台完整，未签名演练包、其他提交、过期或缺失制品均拒绝发布。若原始制品已无法取得，应停止该版本恢复并由发布负责人处理，不能覆盖已存在的版本。来源冻结和市场审核状态由平台记录，流水线只记录执行过程。

### 本机真实运行时验证

在已有兼容 Codex CLI 的机器运行 `cargo build --locked`，再执行：

```sh
python3 tools/smoke-runtime.py --connector target/debug/baijimu-digital-employee-connector --codex /absolute/path/to/codex
```

该检查使用临时应用数据目录，验证 HTTP 鉴权、工作区上下文、独立 app-server 初始化、空登录态、任务创建与读取，以及 Unix 子进程退出。它不登录账号、不发起模型轮次；发布验收仍需在宿主安装后，以员工独立授权完成一次任务、审批和事件回传。
