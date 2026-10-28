# 实时输出流（4.0.0）

2026-10-28 当前用户确认 Connector 对外协议变更与版本 3.0.0 → 4.0.0（MAJOR）。平台记录见 `lowcode/docs/decisions/2026-10-28-local-app-notification-stream.md`。

- 新增 `GET /streams/notifications`（SSE），manifest 声明 `notificationStream` 与宿主能力 `connector.notification-stream.v1`，需要支持该能力的 Bridge Agent（0.15.0 起）。
- **不兼容**：不再上传 `codexNotification` 事件，订阅该事件的消费者必须改用输出流；平台 lowcode-websocket 已在同期版本支持输出流。
- `codexTurnCompleted`、`codexThreadClosed`、`codexThreadArchived` 与 `recentEvents` 方法不变。
