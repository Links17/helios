# Helios MQTT 实施 TODO

## 原则

- 完全复用现有 Helios 组件架构：`main -> channel -> start_xxx loop`。
- 优先走 `SeekRequest`，避免 MQTT 旁路直接改 Docker。
- 协议对齐 `csg_supervisor` 的 topic 与 payload 语义。

## TODO 清单

- [x] 新建 `helios-mqtt` crate 与基础模块拆分
- [x] 建立 releaseControl 到 `SeekRequest` 的映射
- [x] 建立 shadow env 变更语义（`full/delta` + `null/""` 删除）
- [x] 建立 script 请求/响应模型与执行器（超时/输出截断）
- [x] 接入真实 MQTT 客户端（连接、订阅、重连）
- [x] 增加 `helios/src/cli.rs` MQTT 参数
- [x] 在 `helios/src/main.rs` 以 `maybe_start` 风格接入 `helios-mqtt::start`
- [x] 接入 host metrics 触发链路（`get/deviceStatus` -> 立即采样）
- [x] 增加 shadow env 本地持久化（`helios-store` 或独立存储）
- [x] 增加 `update/rejected` 重试与 `409` 自动恢复流程任务
- [x] 补齐 `update/script` topic 发布链路与 E2E 测试
- [x] 增加 release/script/shadow 集成测试
- [ ] 明确周期上报启动策略：首次延迟上报
- [ ] 明确周期上报启动策略：启动立即上报
- [ ] 为 `deviceStatus` 与 `releaseStatus` 提供独立上报间隔配置
