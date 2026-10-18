# 退出码与恢复动作

退出码与 JSON 必须一起判断。非零不等于没有可用数据。

| 退出码 | 常量语义 | 恢复动作 |
|---|---|---|
| 0 | SUCCESS | 读取 data，仍检查 meta.complete |
| 2 | INVALID_ARGUMENT | 根据 leaf Schema 修正参数 |
| 3 | AUTH_REQUIRED | 根据 error.code、challenge_id 和 next_action 登录或续接 |
| 4 | PERMISSION_DENIED | 停止并报告权限边界 |
| 5 | UPSTREAM_ERROR | 报告真实上游或传输错误，仅按 retryable 判断是否允许重试 |
| 6 | STATE_INVALID | 本地存储或锁异常；保留状态并报告 |
| 7 | INCOMPLETE | 保留已有 data；检查 meta.complete、stop_reason 与分页范围 |

## 认证错误

- `CAPTCHA_REQUIRED`：`auth captcha` 返回 `data.captcha_path`；验证使用 `auth verify --challenge-id <ID> --stdin-code --stdin-password`。
- `SECOND_FACTOR_REQUIRED`：脱敏目标在 data；用户同意后 `auth send-code --kind sms` 或 `mail`，随后用同一通道提交 `auth verify --challenge-id <ID> --stdin-code --kind <kind>`。
- `INVALID_CREDENTIALS` 且有 challenge_id：凭据重试，只需 `auth verify --challenge-id <ID> --stdin-password`。
- `AUTH_REQUIRED` 且无挑战：安全登录 `auth login --auth <UID> --stdin-password`。
- `AUTH_FLOW_EXPIRED`：挑战过期或不匹配，不猜测 ID；按返回说明重新登录。

## 不完整结果

`card bills` 返回 `ok=true`、`meta.complete=false` 和退出码 7 时，不是内部故障。保留当前页，按已解析的日期范围与页码继续获取，并向用户明确完整性。不得将失败解释成空集合，也不得假设存在 retry_after_seconds 字段。
