# 数字员工历史读取契约

2026-09-30：线上 manager 会话标识修复后，设备历史读取暴露 Codex app-server 边界问题。用户要求继续修复并完成发布验收。

本机实际 Codex CLI 0.154.0 在未指定 historyMode 时创建 paginated 会话；thread/read(includeTurns=false) 成功，首条消息之前 thread/turns/list 返回 -32600 和明确的未持久化错误。旧 Connector 对任意错误回退到 thread/read(includeTurns=true)，后者又返回 -32601 list_turns is not supported yet，并掩盖首个错误。隔离进程复现了全部步骤。明确指定 legacy 后，空会话返回同一个未持久化错误，产生首轮后历史读取与分页均成功。

## 责任边界与实现

- Connector 拥有 Codex RPC 适配。新建会话显式使用 legacy；不修改已有会话的数据或历史模式。用户传入不受支持的存储模式时在创建前返回清晰错误。
- 仅首屏、CODEX_RPC_ERROR、rpcCode=-32600、消息完整匹配当前 threadId 的“首条消息前尚未持久化”状态映射为空历史。此为 Codex 正常空历史状态的协议适配，不是遇错返回空数组。
- 分页游标错误、会话不存在、存储故障、未知方法、未支持模式和其他错误保持失败。只调用一次 thread/turns/list，移除通用 thread/read 回退，不引入缓存、第二个历史源或错误吞没。
- 修复不改变会话字段、Relay、agent-session 或 lowcode-websocket；执行目录 Connector >=2.0.0 <3.0.0 仍满足。发布客户端本地应用 2.0.1 PATCH。
- 已创建的 paginated 验收空会话保留，不伪造迁移。使用新建 legacy 会话验证完整链路。

依据：官方 [app-server 文档](https://learn.chatgpt.com/docs/app-server#start-or-resume-a-thread) 与本机 0.154.0 隔离 JSON-RPC 实测；不同发行版本的默认值可能不同，因此不能依赖省略参数。
