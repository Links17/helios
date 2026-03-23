# Helios 主机状态采集、Shadow Env 与 MQTT 控制设计文档

## 1. 背景与目标

当前 `helios` 已具备较清晰的组件化架构（`api` / `poll` / `seek` / `report`），但缺少三类能力：

1. 宿主机基础状态采集（CPU、温度、内存、磁盘、IP 等）。
2. 基于 MQTT 协议的设备状态上报与控制通道（参考 `csg_supervisor` 协议）。
3. 基于 Shadow 的容器环境变量同步，以及脚本指令通道。

本设计目标是在**尽量复用现有 Helios 架构风格**前提下，新增上述能力，并保证对现有 HTTP 远端流程最小侵入。

---

## 2. 需求范围

### 2.1 必须实现

- 新增宿主机状态采集模块，支持周期采集和按需触发。
- 新增 MQTT 模块，协议对齐 `csg_supervisor`：
  - 订阅设备状态请求与 release 控制请求；
  - 订阅并处理 Shadow env 主题（`get/accepted`、`update/delta` 等）；
  - 订阅并处理脚本下发主题（`get/script`）；
  - 上报设备状态；
  - 上报脚本执行结果（`update/script`）；
  - 收到 `stop/restart/start` 指令后复用 Helios 状态机能力执行容器控制。
- 给出可落地的实现设计与分阶段计划。

### 2.2 非目标（首期）

- 不直接暴露“宿主机 SSH/terminal 远控”能力（需额外鉴权与审计系统，超出本期）。
- 不替换现有 `helios-remote` HTTP 轮询/上报链路，仅并行增加 MQTT 通道。

---

## 3. 现状学习结论（复用点）

### 3.1 主程序编排模式

- 入口在 `helios/src/main.rs`，通过 `tokio::select!` 并发启动组件。
- 组件间用 `tokio::sync::watch` 进行状态/请求广播：
  - `SeekRequest`：目标状态写入；
  - `PollRequest`：拉取触发；
  - `LocalState`：当前设备状态广播。

### 3.2 状态与上报模式

- `helios-state` 维护设备本地状态，`start_seek` 处理目标应用并更新 `LocalState`。
- `helios-remote::start_report` 订阅 `LocalState` 变化并增量上报（patch diff）。

### 3.3 配置与风格

- CLI/ENV 参数集中在 `helios/src/cli.rs`。
- 配置结构多为强类型结构体 + `serde`。
- 对持久化结构变更极其谨慎（`helios-remote/src/config.rs` 已明确警示），因此 MQTT 配置应与现有身份配置解耦，避免破坏已有序列化兼容性。

### 3.4 OCI 控制能力

- `helios-oci/src/container.rs` 已提供：
  - `start(name)`
  - `stop(name)`
- 暂无显式 `restart`，可由 `stop + start` 组合实现。

### 3.5 与 env/script 相关现状

- 当前 Helios `ServiceConfig` 仅支持 `command` 和 `labels`，未建模 `environment`。
- 当前 Helios 没有 `script` 指令主题处理链路。
- 因此 env/script 需新增模型与运行时模块，但应尽量复用现有 `seek` / `LocalState` / `report` 风格。

---

## 4. MQTT 协议参考（来自 csg_supervisor）

参考目录：`/Users/links/Documents/Project/Seeed/edge/csg_supervisor`

### 4.1 主题约定

- 设备状态上报：`<topicHead>/<fleetId-deviceUUID>/update/deviceStatus`
- 设备状态请求：`<topicHead>/<fleetId-deviceUUID>/get/deviceStatus`
- Release 状态上报：`<topicHead>/<fleetId-deviceUUID>/update/releaseStatus`
- Release 控制/查询：`<topicHead>/<fleetId-deviceUUID>/get/releaseStatus`
- 脚本下发：`<topicHead>/<fleetId-deviceUUID>/get/script`
- 脚本结果上报：`<topicHead>/<fleetId-deviceUUID>/update/script`
- Shadow 获取：`<topicHead>/<fleetId-deviceUUID>/shadow/name/deviceEnv/get`
- Shadow 更新：`<topicHead>/<fleetId-deviceUUID>/shadow/name/deviceEnv/update`
- Shadow 订阅：
  - `.../get/accepted`
  - `.../get/rejected`
  - `.../update/accepted`
  - `.../update/rejected`
  - `.../update/delta`

### 4.2 行为约束

- `qos = 0`
- 发布 `retain = false`
- 订阅 `clean_session = true`（协议文档约束）
- `releaseControl` 指令中 `action` 语义：
  - `1 = start`
  - `2 = stop`
  - `3 = restart`

### 4.3 csg env/shadow 关键语义（设计必须兼容）

- 首次连接应发送空消息到 `shadow.../get` 获取当前影子。
- `get/rejected`：
  - `404` 表示影子不存在，设备需上报当前 `reported.env`（可为空对象）。
  - 非 404 需按 30 秒重试。
- `get/accepted` 若无 `desired` 字段，表示平台期望 `desired` 为空：设备需清空本地 env 并上报。
- `update/delta` 为增量差异：设备按差异更新本地 env。
- 值为 `""` 或 `null` 都表示删除该 env key；删除后上报时该 key 值为 `null`。
- 设备状态变化后上报 `reported.env`，且仅需上报变化键（增量上报）。
- `update/rejected` 失败按 30 秒重试；`409` 版本冲突需先重新 `get` 同步版本后重试更新。
- 协议文档建议：env 下发后重启全部 service（“balena 是这样做的”）。

---

## 5. 目标架构设计

## 5.1 总体原则

- **并行新增，不破坏原链路**：HTTP `poll/report` 与 MQTT 独立运行。
- **沿用 Helios 事件风格**：新增模块也采用 `start_xxx` + channel 驱动。
- **最小侵入**：尽量不扩展 `LocalState` 的核心语义，避免影响现有 API 与远端上报格式。

## 5.2 组件拆分（建议）

新增两个 crate：

1. `helios-host-metrics`
   - 负责宿主机状态采集与标准化字段输出。
2. `helios-mqtt`
   - 负责 MQTT 连接、订阅、协议解析、命令分发、状态上报；
   - 内含 `release` / `shadow_env` / `script` 三个子模块。

主程序 `helios` 只做编排与配置注入。

## 5.3 通道设计

- `watch::channel<HostMetricsSnapshot>`：宿主机指标广播（最新值语义）。
- `mpsc::channel<MqttCommand>`：MQTT 下行控制命令（需逐条执行，使用队列语义）。
- `mpsc::channel<MetricsTrigger>`：按需触发采集（例如收到 `get/deviceStatus` 后立即采样一次）。
- `mpsc::channel<ShadowEnvDelta>`：env 变化请求（含来源 `get/accepted` 或 `delta`）。
- `mpsc::channel<ScriptRequest>`：脚本执行请求（串行或受限并行执行）。

---

## 6. 宿主机状态采集设计

## 6.1 数据模型（对齐 csg）

```rust
pub struct HostMetricsSnapshot {
    pub timestamp_ms: i64,
    pub memory_total_mb: f64,
    pub memory_used_mb: f64,
    pub sd_total_mb: f64,
    pub sd_used_mb: f64,
    pub flash_total_mb: f64,
    pub flash_used_mb: f64,
    pub cpu_temperature_c: f64, // 无法读取时按协议约定填 99999
    pub cpu_used_percent: f64,
    pub local_ip: Vec<String>,
    pub public_ip: Vec<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub lora_modem: Option<String>,
    pub lte: Option<u8>,
    pub gps: Option<u8>,
}
```

说明：

- 首期建议将 `public_ip` 与地理位置采集做成可选能力（默认关闭），避免依赖外网服务。
- 无法采集的字段按协议兼容值填充，不阻塞整体上报。

## 6.2 采集来源建议

- CPU 使用率：`/proc/stat` 或 `sysinfo` crate。
- CPU 温度：`/sys/class/thermal/.../temp`（支持多传感器时取首个有效值）。
- 内存：`/proc/meminfo` 或 `sysinfo`。
- 磁盘：`statvfs` 或 `sysinfo` 文件系统统计。
- IP：本机网卡枚举（IPv4 优先）。

## 6.3 采集策略

- 启动后立即采集一次。
- 默认每 5 分钟采集一次（与 csg 文档一致）。
- 收到 MQTT `get/deviceStatus` 后立即触发一次（旁路触发）。

---

## 7. MQTT 模块设计

## 7.1 配置模型

新增 `MqttConfig`（建议仅来自 CLI/ENV，首期不持久化）：

- `broker_url`
- `topic_head`（如 `sensecapmx`）
- `fleet_id`
- `device_uuid`（默认可复用 Helios UUID）
- `username/password` 或 `tls cert/key/ca`
- `clean_session`（默认 `true`）
- `keep_alive_sec`
- `report_interval_sec`（默认 300）
- `script_exec_timeout_sec`（默认 30）
- `script_max_output_bytes`（默认 64KB）
- `env_apply_strategy`（`full_restart` / `rolling_restart`，默认 `full_restart` 以兼容 csg）
- `script_enable`（默认 `true`）
- `shadow_env_enable`（默认 `true`）

## 7.2 Topic 映射

内部封装 `topics.rs`，保持与 `csg_supervisor/pkg/mqtt/topics.go` 同构：

- `device_status_get()`
- `device_status_update()`
- `release_status_get()`
- `release_status_update()`
- `script_get()/script_update()`
- `shadow_get()/shadow_update()`
- `shadow_get_accepted()/shadow_get_rejected()`
- `shadow_update_accepted()/shadow_update_rejected()/shadow_update_delta()`

## 7.3 消息处理流程

### A. 收到 `get/deviceStatus`

1. 校验 topic 与 payload 中 `fleetId/deviceUUID` 一致；
2. 投递 `MetricsTrigger::Immediate`；
3. 使用最新 `HostMetricsSnapshot` 组装协议 payload；
4. 发布到 `update/deviceStatus`。

### B. 收到 `get/releaseStatus`

- `order.name = "releaseStatus"`：立即上报当前 release 状态；
- `order.name = "releaseControl"`：解析 `services[].name/action`，转换为 `SeekRequest` 目标并投递。

### C. 执行 `releaseControl`

- action=1：目标服务 `started=true`
- action=2：目标服务 `started=false`
- action=3：执行两段目标变更（`false -> true`）
- 执行后立即触发一次 release 状态上报。

说明：为“完全复用 Helios 内核”，MQTT 不直接操作 Docker，统一走 `seek` 状态机。

### D. 收到 Shadow env 消息

1. `get/accepted`：
   - 无 `desired.env`：清空本地 env 存储，应用到容器并上报 `reported.env={}`；
   - 有 `desired.env`：按“完整快照”模式应用（不在 desired 的 key 视为删除）。
2. `update/delta`：按“增量”模式应用（只改变化键，`""/null` 删除）。
3. 本地 env 变化后，发布 `shadow.../update`，仅上报变化键，删除键上报 `null`。
4. `get/rejected` 非 404、`update/rejected` 非 409：30 秒重试。
5. `update/rejected=409`：清空本地 shadow 版本，重新 `get` 后重试 `update`。

### E. 收到 `get/script`

1. 解析 payload（`requestId/timestamp/fleetId/deviceUUID/script`）。
2. 校验：
   - `type=request`；
   - `expireTime` 未过期（建议实现）；
   - topic 与 payload 设备身份一致。
3. 执行 `script.cmd`（兼容 csg：`/bin/sh -c`）。
4. 组装响应：
   - topic：`update/script`
   - 保持同一个 `requestId`
   - `order.name=script`
   - `value.code=exitCode`
   - `value.msg=stdout/stderr 截断输出`
5. 发布响应（`qos0/retain=false`）。

## 7.4 Shadow Env 与 Helios 状态模型对齐

由于当前 Helios `ServiceConfig` 尚无 `environment` 字段，建议分两阶段：

- 阶段 A（兼容优先）：
  - 新增 `helios-env-store`（或复用 `helios-store`）保存 shadow env；
  - env 变更时通过统一“容器重建更新”路径应用（策略默认 `full_restart`）；
  - 与 csg 行为保持一致（下发 env 后重启 service）。
- 阶段 B（内核收敛）：
  - 扩展 `ServiceConfig` 增加 `environment`；
  - 将 env 变更纳入 `SeekRequest -> planner -> tasks`，实现与 release 同一路径收敛。

---

## 8. 与 Helios 现有架构的集成方式

## 8.1 `main.rs` 组装

在现有 `tokio::select!` 中按“可选组件”模式并发启动：

- `maybe_start(mqtt_config, |cfg| helios_mqtt::start(...))`
- `maybe_start(host_metrics_config, |cfg| helios_host_metrics::start(...))`

保持与当前 `start_poll/start_report/start_seek` 一致的启动风格，便于后续维护。

## 8.2 与 `LocalState` 的关系

- 不直接改动 `LocalState` 结构（首期），减少对 `helios-api` 与 `helios-remote` 的连锁影响。
- MQTT 的设备状态上报优先依赖 `HostMetricsSnapshot`，release 状态可复用 `LocalState`（容器状态）。
- script 执行结果通过 MQTT 响应 topic 上报，不进入 `LocalState` 主结构。
- env 变更事件可额外建立 `watch::channel<EnvRuntimeState>` 供观测与调试。

## 8.3 部署形态

- 继续保持**单一主二进制**（`helios`）；
- 新增 crate 作为库被 `helios` 链接，不新增独立守护进程，降低运维复杂度。

---

## 9. 安全与边界

- 强制校验 topic 与 payload 的 `fleetId/deviceUUID` 一致性，不一致直接丢弃。
- 只允许 `releaseControl` 白名单动作（1/2/3）。
- script 仅在 `script_enable=true` 时启用，且必须受限：
  - 执行超时；
  - 输出长度上限；
  - 并发上限；
  - 可选命令白名单（建议生产默认开启）。
- 不开放 SSH/terminal 直控。
- 对每条控制命令记录审计日志（requestId、service、action、result、error）。
- 对无效容器名、未知 action、反序列化失败、脚本超时进行结构化告警日志。

---

## 10. 错误处理与可靠性

- MQTT 连接：自动重连 + 指数退避。
- 订阅失败：重试并上报码流日志。
- 采集失败：单字段降级，不影响整包上报。
- 命令执行失败：返回错误状态并触发一次 release 状态刷新，确保云端可见最终状态。
- script 执行失败：保持 `requestId` 回包，`code!=0`，并附错误摘要。
- shadow `get/update` 失败：遵循 csg 语义重试（30 秒）与 409 版本冲突恢复流程。

---

## 11. 代码落地建议（文件级）

## 11.1 新增

- `helios-host-metrics/Cargo.toml`
- `helios-host-metrics/src/lib.rs`
- `helios-host-metrics/src/model.rs`
- `helios-host-metrics/src/collector.rs`
- `helios-host-metrics/src/runtime.rs`

- `helios-mqtt/Cargo.toml`
- `helios-mqtt/src/lib.rs`
- `helios-mqtt/src/config.rs`
- `helios-mqtt/src/topics.rs`
- `helios-mqtt/src/protocol.rs`
- `helios-mqtt/src/client.rs`
- `helios-mqtt/src/handlers.rs`
- `helios-mqtt/src/executor.rs`
- `helios-mqtt/src/reporter.rs`
- `helios-mqtt/src/shadow_env.rs`
- `helios-mqtt/src/script.rs`
- `helios-mqtt/src/script_exec.rs`
- `helios-mqtt/src/env_store.rs`（或整合到 `helios-store`）

## 11.2 修改

- `Cargo.toml`（workspace members + dependencies）
- `helios/Cargo.toml`（引入新 crate）
- `helios/src/cli.rs`（新增 MQTT/metrics/shadow/script 相关参数）
- `helios/src/main.rs`（新增 channel 与启动编排）
- `helios-state/src/models/service/config.rs`（阶段 B：新增 `environment` 字段）
- `helios-oci/src/container.rs`（阶段 B：`ContainerConfig` 增加 `env`）

---

## 12. 分阶段实施计划

### Phase 1：Host Metrics 基础能力

- 完成 `helios-host-metrics` 采集能力与定时循环；
- 在本地日志中输出采样结果；
- 增加单元测试（数值合法性、降级逻辑）。

### Phase 2：MQTT 通道与 deviceStatus/releaseControl

- 完成 MQTT 连接、订阅、设备状态上报；
- 打通 `get/deviceStatus -> 立即采集 -> update/deviceStatus`。
- 打通 `get/releaseStatus -> SeekRequest` 控制链路。

### Phase 3：Shadow Env 同步链路

- 打通 shadow topic 订阅与解析；
- 实现 `get/accepted`、`update/delta`、`rejected` 重试与 409 冲突恢复；
- 将 env 变化应用到容器并回报 `reported`。

### Phase 4：Script 指令链路

- 打通 `get/script -> execute -> update/script`；
- 实现超时/输出截断/并发控制；
- 补充审计日志与失败回包策略。

### Phase 5：联调与灰度

- 与云端协议联调（topic/payload/频率）；
- 小流量灰度并观察误控、重连、采集稳定性、脚本安全风险；
- 完成回滚开关（CLI/ENV 级 feature toggle）。

---

## 13. 验收标准（Definition of Done）

- 能按 5 分钟周期稳定上报主机状态；
- 能在收到 `get/deviceStatus` 后 3 秒内回包上报；
- 能正确执行 `releaseControl` 的 1/2/3 动作；
- 动作执行后状态上报与容器真实状态一致；
- 能按 csg 规则处理 shadow env 的 get/delta/update/rejected 流程；
- env 删除语义兼容 `""/null`；
- 能接受并执行 `get/script`，并在 `update/script` 回包执行结果；
- MQTT 异常重连后可自动恢复订阅与上报；
- 现有 `api/poll/seek/report` 功能与测试不回归。

---

## 14. 风险与应对

- **风险：容器命名与服务名不一致**  
  应对：建立服务名到容器名映射策略（优先 label，再 fallback 名称）。

- **风险：CPU 温度在部分设备不可读**  
  应对：协议降级值 `99999` + 指标可观测日志。

- **风险：公网 IP 获取依赖外部网络**  
  应对：默认关闭公网 IP 拉取，作为可选配置。

- **风险：误将 MQTT 配置持久化进关键身份结构**  
  应对：MQTT 配置独立结构，不混入 `RemoteConfig/ProvisioningConfig`。

- **风险：script 模块带来命令执行安全面**  
  应对：默认超时、输出上限、并发上限、可选命令白名单与审计日志。

- **风险：env 变更触发全量重启带来业务抖动**  
  应对：首期兼容 `full_restart`，后续支持 `rolling_restart` 策略。

---

## 15. 结论

该方案与 Helios 现有代码风格一致：  
**组件化 + channel 驱动 + 主程序编排**。  
同时满足：

- 宿主机状态采集；
- MQTT 协议兼容接入；
- release 控制复用 Helios `seek` 内核；
- Shadow env 与 script 指令能力补齐；
- 安全边界清晰、可逐步灰度上线。
