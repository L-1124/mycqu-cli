# mycqu-cli 设计

状态：设计提案，尚未实现。本文中的命令、错误码和新增 Rust API 均为拟定契约，不代表现有功能。

## 1. 目标

提供供 CQU Agent 调用的 Rust 原生 CLI，命令名为 `mycqu`。复用 `rsmycqu` 的校园协议与数据模型，提供可发现、非交互、结构化、可跨进程复用认证的校园查询能力。

借鉴 DingTalk Design CLI 的任务级命令和共享上下文，不照搬常驻交互工作台或 Factory/Hook 架构。

成功标准：Agent 能发现命令、确定当前身份、完成查询；认证中断可以继续；上游错误不会被伪装成空数据；凭据不会进入输出与日志。

非目标：

- 不重写校园 HTTP SDK。
- 不在 CLI 内实现自然语言理解。
- 不默认建设 MCP 服务、守护进程、插件系统或多 Provider 框架。
- 不实现验证码自动识别或二次验证绕过。
- 不把可选课程查询宣称为选课提交能力。
- 不承诺尚未启用的图书馆模块。

## 2. 已核实的基础

本地依赖为 `../rsmycqu`，包版本 `0.4.4`，Rust edition 为 2024，许可证为 `AGPL-3.0-only`。建议 CLI 使用兼容的 `AGPL-3.0-only`，发布时保留许可证及相应源码义务。

当前源码提供：

| 领域 | 已有接口或类型 |
| --- | --- |
| 认证 | `sso::login`、`captcha_image`、`send_second_factor_code`、`submit_second_factor`、`logout` |
| 会话 | `Session`、`Client`、Cookie 注入与按 URL 读取、服务授权信息、瑞数处理 |
| 用户 | `User::fetch_self`、`access_mycqu` |
| 学期 | `CQUSession`、`CQUSessionInfo` 的列表、当前学期及详情查询 |
| 课表 | `CourseTimetable::fetch_curr`、`fetch_enroll` |
| 成绩 | `Score::fetch_self`、`GPARanking::fetch_self` |
| 课程目录 | `EnrollCourseInfo::fetch_all`、`EnrollCourseItem::fetch_all` |
| 校园卡 | `access_card`、`Card::fetch_self`、`Card::fetch_bill` |
| 宿舍 | `EnergyFees::fetch_self` |

默认 features 为 `sso`、`mycqu`，校园卡需显式启用 `card`。

现有限制：

- `Session` 不是可完整序列化和恢复的公开状态对象。
- `AccessInfos` 可序列化，但不等于完整会话。
- `Session::cookies(url)` 只返回该 URL 对应的 Cookie 头，不能完整保留 Cookie 的 domain、path、expiry 等属性。
- 登录状态、服务授权和瑞数上下文共同影响后续调用，仅保存 Cookie 字符串不足以证明跨进程恢复有效。
- `LoginResult` 已区分成功、凭据错误、图形验证码和二次验证；CLI 需要把这些结果映射为可恢复流程。
- README 中示例和能力说明不完全反映当前源码，实施以实际接口和联调结果为准。

## 3. 职责边界

| 层 | 职责 |
| --- | --- |
| rsmycqu | 校园请求、登录协议、响应解析、数据模型、可恢复会话及认证上下文 |
| mycqu-cli | 命令参数、任务编排、筛选、安全存储、锁、JSON 输出、错误映射及退出码 |
| Agent | 理解意图、发现命令、请求用户完成认证、组织回答 |

CLI 不直接拼校园请求，不访问库的私有字段，不自行维护另一份瑞数或 SSO 实现。库不感知 CLI 的目录、命令、stdout 或终端提示。

## 4. 依赖库支持

`rsmycqu` 所需支持项、优先级及验收条件统一维护在 [rsmycqu TODO](../rsmycqu/TODO.md)，本文不重复维护。

## 5. 命令面

```text
mycqu
  schema [command-path]
  auth login
  auth status
  auth captcha
  auth send-code
  auth verify
  auth logout
  user show
  term list
  term current
  term show
  schedule list
  score list
  score gpa
  course list
  course classes
  card balance
  card bills
  dorm balance
```

命令名称及 flags 在实现前经 Schema 设计确定。示例：

```bash
mycqu schema schedule list
mycqu schedule list --date 2026-10-19
mycqu schedule list --term current --week 8
mycqu score list --term current
mycqu course classes --course-id <真实课程ID>
```

语义约束：

- 缺省查询当前登录身份，不默认查询他人。
- `current` 通过真实学期接口解析，不硬编码年份与 ID。
- 日期采用 `Asia/Shanghai`，输出写明时区和已解析的查询范围。
- `--date` 与学期、周次条件的冲突应拒绝或明确约束，不静默覆盖参数。
- 成绩学期筛选若依赖本地过滤，应在文档说明；不虚构服务端过滤能力。
- `card bills` 的时间范围、分页及是否完整遵循真实上游契约。
- 所有业务命令默认非交互；需要人工完成认证时返回结构化错误后退出。

## 6. 认证与本地存储

### 6.1 流程

1. `auth login` 使用安全输入接收账号和密码。
2. 成功后保护并保存会话；密码不持久化。
3. 需要图形验证码时保存待完成上下文，返回 opaque `challenge_id`。
4. `auth captcha` 获取与该流程绑定的图片，返回受限本地文件引用；不把原始认证上下文暴露给 Agent。
5. 需要二次验证时返回可用通道及脱敏目标；`auth send-code` 作为显式副作用动作发送验证码。
6. `auth verify` 在原上下文提交验证码，必要时安全接收再次提交所需的密码。
7. 成功后移除已消费的本地挑战状态，保存可供下一进程使用的会话。

`auth status` 默认只描述本地状态；显式在线检查应清楚标注所验证的服务及时间。`auth logout` 需清楚表达服务端退出与本地清理结果，不因某一步失败而宣称全部完成。

### 6.2 存储与安全

- Windows 使用系统支持的凭据保护机制及受限存储，具体库在实施时选定；不得自制加密或明文落盘替代。
- 状态放在用户级应用数据目录，不放进仓库，不依赖当前工作目录。
- 对保存会话、待完成流程及相关图片应用访问限制。
- 同一身份的状态更新使用锁与原子写入，防止并发覆盖和半写入。
- 快照、挑战和图片有明确的有效期及清理规则。
- 密码和验证码不接受普通命令行值参数，使用隐藏终端输入或 stdin。非交互缺少输入时立即失败。
- 不在 stdout、stderr、日志、崩溃信息中打印密码、Cookie、token、完整认证响应或未脱敏联系方式。
- 不添加自动发送验证码、无限重登或无限网络重试。

初期只支持一个显式选定的身份，不隐式按最近使用账号切换。未来确需多身份时再设计隔离的 profile 与默认选择规则。

## 7. Agent 输出契约

默认 JSON；可显式选择面向人类的文本格式。stdout 只包含一个结果对象，诊断进入已脱敏的 stderr。Schema 版本不等同于程序版本。

成功示例：

```json
{
  "schema_version": 1,
  "ok": true,
  "data": {
    "items": []
  },
  "meta": {
    "complete": true,
    "timezone": "Asia/Shanghai"
  }
}
```

空列表只能表示成功查询且没有匹配项。只有实际完成查询时才标记 `complete: true`。完整性未知或部分失败时标记 `complete: false` 并说明原因；只有上游确实存在时才返回 `next_cursor` 等分页字段。

失败示例：

```json
{
  "schema_version": 1,
  "ok": false,
  "error": {
    "code": "CAPTCHA_REQUIRED",
    "message": "需要人工输入图形验证码",
    "retryable": false,
    "challenge_id": "opaque-id",
    "next_action": "auth captcha"
  }
}
```

拟定错误族：

| 错误码 | 语义 |
| --- | --- |
| `INVALID_ARGUMENT` | 参数缺失、冲突或无效 |
| `AUTH_REQUIRED` | 未登录或认证失效 |
| `INVALID_CREDENTIALS` | 账号或密码错误 |
| `CAPTCHA_REQUIRED` | 需要人工完成图形验证码 |
| `SECOND_FACTOR_REQUIRED` | 需要短信或邮件验证 |
| `AUTH_FLOW_EXPIRED` | 待完成认证流程过期 |
| `PERMISSION_DENIED` | 当前身份无相应服务或数据权限 |
| `NETWORK_TIMEOUT` | 请求超时 |
| `UPSTREAM_ERROR` | 上游异常响应 |
| `RESPONSE_INVALID` | 响应结构不符合协议 |
| `STATE_INVALID` | 本地状态损坏或版本不兼容 |

退出码采用小而稳定的分类：0 为完整成功，2 为参数错误，3 为需要认证或人工动作，4 为权限失败，5 为网络或上游失败，6 为本地状态失败，7 为返回了不完整结果。细分原因以 JSON 错误码和 meta 为准；不完整结果可保留有效数据，但不得以 0 退出。

`retryable` 只表示适合自动重试；需要人工验证码不属于自动可重试。请求配置明确超时，对可能有副作用的动作不做盲目重试。

## 8. Schema 与 Agent skill

`mycqu schema` 返回命令目录，`mycqu schema <command-path>` 返回叶子命令的：

- 参数类型、必填项、互斥约束及默认语义；
- 输入与输出 JSON Schema；
- 认证前置条件；
- 副作用与所需确认；
- 可能的错误码与后续动作。

帮助与 Schema 复用命令元数据，不维护两套手写命令清单。输入缺失和参数解析失败也要符合输出契约。

Agent skill 只介绍能力发现、身份选择、认证续接、隐私限制与完整性判断，不复制整套接口手册。发送验证码、退出登录及披露敏感业务数据需符合调用环境的用户授权与确认要求；Schema 描述不能替代实际授权。

## 9. 工程组织

开发阶段：

```toml
[dependencies]
rsmycqu = { path = "../rsmycqu", features = ["card"] }
```

发布阶段使用包含必要变更的固定版本或固定提交，提交锁文件。只添加命令解析、异步运行、JSON 和安全存储等真实需要的依赖，不自行实现其成熟替代品。

建议源码结构：

```text
src/
  main.rs
  cli.rs
  output.rs
  auth.rs
  store.rs
  commands/
    user.rs
    term.rs
    schedule.rs
    score.rs
    course.rs
    card.rs
```

这是职责划分，不要求预先创建空模块。直接调用库接口，只有出现真实重复逻辑时再提取共享函数。

## 10. 实施顺序与验收

| 阶段 | 交付 | 验收 |
| --- | --- | --- |
| 1 | 输出、参数错误、Schema | 程序可解析成功和失败输出；stdout 无日志混入 |
| 2 | 库快照及认证续接、安全存储 | 独立进程登录与查询；验证码和二次验证可跨进程继续 |
| 3 | 用户、学期、课表、成绩 | 身份与学期解析正确；行课周次和过滤边界正确 |
| 4 | 课程目录、校园卡、宿舍 | 服务授权分别验证；范围和完整性不误报 |
| 5 | Agent skill、文档、发布 | Agent 可自行发现命令并在认证中断时正确停止和续接 |

验收分为无需真实账号的确定性回归验证，以及经授权的真实校园联调。优先保留能发现消费者可见问题的测试，不测试源代码文本或单纯转发行为。

真实联调至少覆盖：

- 一个进程登录，另一个进程查询当前用户及课表。
- 会话失效后业务命令返回认证错误，不输出成功空列表。
- 图形验证码及二次验证按实际可触发场景续接；未触发场景明确记录为未验证。
- 周次边界、不行课周及日期映射不确定的场景。
- 校园卡及宿舍查询按各自权限完成只读验证。
- 多进程状态更新和失败写入不会损坏已保存状态。
- 输出及错误诊断中不存在认证秘密。

编译通过不等于认证或业务联调成功。无法取得真实账号授权时，完成可独立验证的工作，并明确列出尚未验证的真实路径，不伪造结果。

## 11. 实施前仍需核实

- 现有 Cookie 容器能否支持完整快照；若不能，库内采用哪种可保留 Cookie 属性的存储实现。
- 瑞数状态跨进程恢复的最小必要集合，以及失效后的合法重建路径。
- 认证在线检查能否完全复用现有接口，以及是否存在可识别的无权限状态。
- 教务学期信息对实际教学周和调课的覆盖程度。
- 校园卡账单等接口的真实分页与范围限制。
- Windows 凭据保护实现的依赖、容量限制及安装环境支持。

这些核实项不能通过凭空增加命令、保存少数 Cookie 或返回默认空值来替代。
