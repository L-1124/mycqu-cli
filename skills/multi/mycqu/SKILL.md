---
name: mycqu
description: 重庆大学 (CQU) 校园信息与教务查询 CLI。Use when 用户需要查询重大课表、成绩、GPA、选课名单、校园卡余额/流水、宿舍电费、当前学期信息或登录管理。不用于瑞数纯算逆向（走 cqu-reverse）、非 CQU 校园业务、自动化选课抢课。命令前缀：mycqu。
metadata:
  cli_version: ">=0.1.0"
  category: campus
  requires:
    bins:
      - mycqu
---

# mycqu 执行契约

## 最小执行契约

- 只调用 `mycqu` 二进制；所有命令原生输出结构化 JSON，按返回中的 `ok` 和 `data` 判断结果。
- 绝不猜测命令、flag 或参数值。已在 Golden Routes 列出的命令直接执行；参数、约束不明时查询 leaf Schema：`mycqu schema <subcommand>`；禁止连续猜测替代命令。
- 严禁在命令中拼接明文密码或将用户密码记录在上下文与输出中；需要登录时使用受控标准输入或交互。
- 认证中断（图形验证码/二次验证短信）属于预期状态机，按返回的 `challenge_id` 引导用户推进，禁止盲目重试。
- 日期遵循 `Asia/Shanghai` 时区；写操作或带副作用操作须经用户确认。

## Golden Routes（高频意图优先）

| 用户意图 | 首选命令入口 | 关键边界 / 说明 |
|---|---|---|
| 查看指定日期课表 | `mycqu schedule list --date <YYYY-MM-DD>` | 日期格式严格为 YYYY-MM-DD；查今天请填今天实际日期 |
| 查看特定教学周课表 | `mycqu schedule list --week <周次>` | 周次为 1 到 25 的整数（如 `--week 8`） |
| 查看当前学期完整课表 | `mycqu schedule list --term current` | 返回学期全量排课，按星期和节次排列 |
| 查看当前学期成绩单 | `mycqu score list --term current` | 仅返回已归档出分的科目列表 |
| 查看历年总绩点与排名 | `mycqu score gpa` | 返回 GPA、加权均分及排名摘要 |
| 查询校园卡余额 | `mycqu card balance` | 需已授权 card 服务；返回即时余额与卡状态 |
| 查询校园卡消费账单 | `mycqu card bills --start-date <YYYY-MM-DD> --end-date <YYYY-MM-DD>` | 必须传入起止日期，可选 `--page 1 --row 50` |
| 查询宿舍水电费余额 | `mycqu dorm balance --room <房间号>` | 房间号如 `梅五-101`，虎溪校区加 `--is-huxi` |
| 查询当前学期及教学周信息 | `mycqu term current` | 返回学期 ID、校历起始日期、当前所在周次 |
| 检查本地登录有效性 | `mycqu auth status [--online]` | 检查本地会话凭据及各微服务授权有效性 |

## Schema 渐进查询契约

当业务命令超出 Golden Routes 或具体参数约束不明时：

1. **查具体 Leaf 契约**：`mycqu schema "schedule list"`
2. **查一级模块命令面**：`mycqu schema schedule`
3. **查顶层概览**：`mycqu schema`

- 发现参数契约后立即调用业务命令，不要把 Schema 发现结果当成业务查询结果。
- 遇到 `unknown flag`：读取一次对应子命令 help 修正；遇到 `unknown command` 立即停止，向用户报告契约漂移，禁止盲目重试。

## 认证与验证码中断流（Challenge 状态机）

当命令返回退出码 `3`，且 `error.code` 为 `CAPTCHA_REQUIRED` 或 `SECOND_FACTOR_REQUIRED` 时：

1. **提取挑战 ID**：从响应中的 `error.challenge_id` 获取 `challenge_id`。
2. **分支处置**：
   - `CAPTCHA_REQUIRED`：说明需要图形验证码。
     - 运行 `mycqu auth captcha` 读取生成的验证码图片路径（`data.captcha_path`）。
     - 提示用户查看图片并提供验证码字符与密码。
     - 提交验证：`mycqu auth verify --challenge-id <ID> --stdin-code --stdin-password`。
   - `SECOND_FACTOR_REQUIRED`：说明需要二次身份验证（短信/邮箱）。
     - 查看 `data` 中的脱敏目标（如 `masked_phone` 或 `masked_email`）。
     - 用户同意后触发发码：`mycqu auth send-code [--kind sms|mail]`（默认 `sms`）。
     - 收集用户验证码后提交：`mycqu auth verify --challenge-id <ID> --stdin-code [--kind sms|mail]`。
3. 验证成功后，重新执行之前被中断的业务命令。
## 错误最短路径

| 错误特征 / 退出码 | 根因 | 最短恢复动作 |
|---|---|---|
| `AUTH_REQUIRED` (3) | 未登录或认证失效 | 使用 `mycqu auth login --auth <UID>` 安全登录 |
| `INVALID_CREDENTIALS` (3)，附 challenge_id | 凭据需要重试 | 经用户提供新密码后执行 `auth verify --challenge-id <ID> --stdin-password`；不读取验证码 |
| `CAPTCHA_REQUIRED` / `SECOND_FACTOR_REQUIRED` (3) | 认证挑战 | 按上述流程续接，使用最新挑战 ID |
| `INVALID_ARGUMENT` (2) | 参数错误 | 查询具体 leaf Schema 后修正 |
| `PERMISSION_DENIED` (4) | 权限不足 | 停止并说明权限边界 |
| `STATE_INVALID` (6) | 本地状态异常 | 保留现有数据，报告错误，不盲目重试 |
| `ok=true`、`meta.complete=false` (7) | 结果不完整 | 保留已取得数据，说明范围及 stop_reason；账单按页继续 |

## 渐进展开子手册

- 认证状态机与凭据安全：[auth-lifecycle.md](references/auth-lifecycle.md)
- 退出码与错误码全集：[error-codes.md](references/error-codes.md)
- 课表时间与学期语义：[schedule-semantics.md](references/schedule-semantics.md)
