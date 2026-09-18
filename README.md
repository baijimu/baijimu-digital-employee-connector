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
