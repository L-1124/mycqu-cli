# 认证生命周期与挑战状态机 (Auth Lifecycle & Challenge State Machine)

## 1. 认证安全原则

1. **密码绝不持久化**：用户明文密码仅在内存中用于执行 SSO 认证交换，不写入磁盘配置、日志或返回结果中。
2. **会话加密保存**：登录成功后的 Cookie Jar 与服务授权上下文通过平台原生机制（Windows DPAPI 或权限受控文件）保存在用户应用数据目录中（`~/.mycqu/session.json`）。
3. **非交互式优先**：当 CLI 在无终端或 Agent 托管环境下调用时，遇挑战直接返回结构化挑战元数据，而非阻塞或挂起进程。

## 2. 状态机流转流程

```mermaid
stateDiagram-v2
    [*] --> 离线检查: mycqu auth status
    离线检查 --> 登录有效: session_persisted=true
    离线检查 --> 登录发起: mycqu auth login --auth <UID> --stdin-password
    
    登录发起 --> 登录成功: 直接通过 (退出码 0)
    登录发起 --> 挑战状态: 遇到验证码 / 2FA (退出码 3)
    
    挑战状态 --> 图形验证码: error.code = "CAPTCHA_REQUIRED"
    图形验证码 --> 获取图片: mycqu auth captcha
    获取图片 --> 提交验证: mycqu auth verify --challenge-id <ID> --stdin-code --stdin-password
    
    挑战状态 --> 二次验证: error.code = "SECOND_FACTOR_REQUIRED"
    二次验证 --> 触发发码: mycqu auth send-code [--kind sms|mail]
    触发发码 --> 提交验证: mycqu auth verify --challenge-id <ID> --stdin-code [--kind sms|mail]
    
    提交验证 --> 登录成功: 校验通过，写入会话
    提交验证 --> 挑战状态: 验证码错误，返回新 challenge_id
```

## 3. Challenge 响应格式说明

当命令需要续接时，退出码为 `3`，标准 JSON 返回示例：

```json
{
  "schema_version": 1,
  "ok": false,
  "data": {
    "stage": "second_factor",
    "sms_available": true,
    "mail_available": false,
    "masked_phone": "138****1234",
    "masked_email": null
  },
  "error": {
    "code": "SECOND_FACTOR_REQUIRED",
    "message": "Second factor verification required. Call 'auth send-code' or submit code with 'auth verify --challenge-id <id> --stdin-code'.",
    "retryable": false,
    "challenge_id": "cqu-c1f7a0b39e424f2ab601837492cfa192",
    "next_action": "auth send-code"
  },
  "meta": {
    "complete": false,
    "timezone": "Asia/Shanghai"
  }
}
```

Agent 读取到此格式后，从 `error.challenge_id` 提取挑战 ID，询问用户后通过 `auth send-code` 与 `auth verify --challenge-id <ID> --stdin-code` 完成闭环。
