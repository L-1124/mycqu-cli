# mycqu-cli

[![Rust 2024](https://img.shields.io/badge/Rust-2024%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPLv3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Agent Skills](https://img.shields.io/badge/Agent%20Skills-Ready-green?logo=openai)](https://agents.md/)
[![Platform: Windows](https://img.shields.io/badge/Platform-Windows%20(DPAPI)-0078D6?logo=windows)](https://learn.microsoft.com/en-us/windows/win32/seccrypto/cryptprotectdata)

[English](README.md) · [简体中文](README_zh.md)

`mycqu` 是专为重庆大学（CQU）校园微服务打造的原生、Agent 优先命令行工具。专为人类开发者及自主 AI Coding Agent（Claude Code、Cursor、Codex、OpenCode 等）设计，将重大统一身份认证（SSO）、教务排课、学业成绩、一卡通以及宿舍水电费等业务整合进非交互、确定性的结构化接口中。

---

## 核心特性

- **Agent 契约优先**：所有命令均统一输出标准 JSON 外层信封（`ApiResponse<T>`），具备严格的进程退出码契约（`0`, `2`, `3`, `4`, `5`, `6`, `7`）与时区标记（`Asia/Shanghai`），全程支持无头自动化与非交互执行。
- **三层渐进式 Schema 自省**：内建三级 Schema 反射（`mycqu schema [command]`），彻底消除大语言模型对参数、Flag 和约束的猜测与幻觉。
- **零信任认证与挑战状态机**：集成底层 SSO 认证与瑞数 6 动态防御算法，使用 Windows DPAPI 本地加密存储会话 Cookie；面对图形验证码与短信/邮箱二次验证（2FA）采用非阻塞状态机流转。
- **内嵌离线 Agent Skills 引擎**：原生内置 multi 与 mono 模式的 Agent Skills，通过 `mycqu skill setup` 可一键分发到 `~/.agents/skills` 以及各大检测到的 Agent 目录，具备目录级 SHA256 审计与覆盖前防灾备份机制。
- **校园全业务查询覆盖**：
  - 👤 **个人档案**：学生/教工身份识别与基本信息核验。
  - 📅 **学期校历**：当前学期、历史学期列表与教学周次换算。
  - 📖 **课程表与排课**：日期到教学周精准映射、节次作息与上课教室分配。
  - 📊 **成绩与绩点**：学期成绩单、历年总加权均分、GPA 及班级/专业/年级官方排名。
  - 💳 **校园一卡通**：实时卡余额与多条件分页消费流水对账。
  - ⚡ **宿舍水电费**：宿舍房间实时电表读数与剩余电费余额。

---

## 快速上手

### 环境要求

- [Rust](https://rustup.rs/) 工具链（1.89+，支持 2024 edition）
- Windows 系统环境（用于 DPAPI 凭据安全保护）
- 自动通过 Git 拉取依赖：[rsmycqu](https://github.com/L-1124/rsmycqu)

### 安装

将 `mycqu` 编译并安装至系统的本地 Cargo 可执行目录：

```bash
cargo install --path .
```

检查版本验证安装：

```bash
mycqu --version
```

### 首次登录

使用你的统一身份认证账号（学工号）发起认证：

```bash
mycqu auth login --auth <你的学号或工号>
```

> [!NOTE]
> 密码通过终端安全掩码输入或从标准输入管道 `--stdin-password` 传入。明文密码绝不落盘、不记入日志，也不会暴露在 JSON 响应中。会话 Cookie Jar 经由 Windows 数据保护接口（DPAPI）加密安全保存。

若触发了图形验证码或短信/邮箱二次验证，CLI 不会挂起进程，而是返回携带 opaque `challenge_id` 的结构化错误并以退出码 `3` (`AUTH_REQUIRED`) 退出，供人类或 Agent 续接：

```bash
# 触发图形验证码时：
mycqu auth captcha
mycqu auth verify --challenge-id <ID> --stdin-code --stdin-password

# 触发短信/邮箱二次验证时：
mycqu auth send-code --kind sms
mycqu auth verify --challenge-id <ID> --stdin-code --kind sms
```

随时确证本地登录状态：

```bash
mycqu auth status [--online]
```

---

## 常用查询命令与调用示例

所有业务命令均输出结构化 JSON，便于机器解析与终端管道编排：

### 课表与上课安排

```bash
# 查询指定日期的课程（内部自动映射到学期与所在教学周，精准过滤当天课程）
mycqu schedule list --date 2026-10-19

# 查询第 7 周的完整周课表
mycqu schedule list --week 7

# 查询当前学期的全量排课条目
mycqu schedule list --term current
```

### 成绩单与学业排名

```bash
# 查看当前激活学期的成绩清单
mycqu score list --term current

# 查询累计 GPA、加权均分及班级/专业/年级排名
mycqu score gpa
```

### 校园卡与水电缴费

```bash
# 查询当前校园卡即时可用余额
mycqu card balance

# 查询指定日期范围内的消费账单流水
mycqu card bills --start-date 2026-10-01 --end-date 2026-10-18 --page 1

# 查询宿舍电表读数与剩余电费（如梅五-101）
mycqu dorm balance --room 梅五-101
```

### 接口 Schema 渐进自省（防大模型幻觉）

```bash
# 第 1 级：列出所有可用业务命令全景
mycqu schema

# 第 2 级：查询模块组级命令概览（如课表模块）
mycqu schema schedule

# 第 3 级：查询具体叶子命令的参数定义、互斥约束与确认语义
mycqu schema "schedule list"
```

---

## Agent Skills 生态集成

`mycqu` 内部打包了符合 [Agent Skills](https://agents.md/) 规范的预制技能，支持 Claude Code、Cursor、Codex、OpenCode 等 AI 工具直接调用：

```bash
# 仅预览安装计划与备份动作（零磁盘写入副作用）
mycqu skill setup --dry-run

# 一键安装技能到 ~/.agents/skills 及已探测到的 Agent 目录
mycqu skill setup --yes

# 校验已安装技能的文件哈希，检测是否有本地契约漂移
mycqu skill check

# 安全卸载受管技能并同步清理状态
mycqu skill uninstall --yes
```

> [!TIP]
> `mycqu skill setup` 会优先在各 Agent 工具目录（如 `~/.cursor/skills/mycqu`、`~/.claude/skills/mycqu`）创建指向规范根目录的符号链接（Symlink），在权限受限时自动平滑回退为安全物理复制。如果目标路径已存在用户自定内容，系统在覆盖前会自动归档至 `~/.mycqu/skill-backups/<时间戳>/`。

---

## 统一响应信封与退出码契约

所有命令均返回符合 `ApiResponse<T>` 规范的一致性 JSON 外层包装：

```json
{
  "schema_version": 1,
  "ok": true,
  "data": { ... },
  "error": null,
  "meta": {
    "complete": true,
    "timezone": "Asia/Shanghai",
    "count": 1
  }
}
```

### 进程退出码速查表

| 退出码 | 标识常量 | 语义说明 | Agent / 自动化处理动作 |
|---|---|---|---|
| `0` | `EXIT_SUCCESS` | 执行成功 | 读取 `data`，同时检查 `meta.complete` |
| `2` | `EXIT_INVALID_ARGUMENT` | 参数语法错误或互斥参数冲突 | 查阅 `mycqu schema <command>` 修正传入参数 |
| `3` | `EXIT_AUTH_REQUIRED` | 未登录、会话过期或需要挑战验证 | 根据 `error.code` 与 `error.challenge_id` 推进认证状态机 |
| `4` | `EXIT_PERMISSION_DENIED` | 权限不足或未开通对应微服务授权 | 终止流程并向用户说明服务权限边界 |
| `5` | `EXIT_UPSTREAM_ERROR` | 教务/一卡通等校园上游系统报错或维护 | 向用户报告上游维护，仅当 `retryable=true` 时退避重试 |
| `6` | `EXIT_STATE_INVALID` | 本地存储锁竞争或状态文件异常 | 检查系统读写权限或重试瞬态文件锁 |
| `7` | `EXIT_INCOMPLETE` | 分页数据未拉取完毕（`complete: false`） | 保留当前已取数据，按页码游标继续推进 |
