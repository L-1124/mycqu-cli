---
name: mycqu
description: 重庆大学 (CQU) 校园服务与逆向工程单体集成技能。Use when 用户需要跨模块同时查询教务与底层协议诊断。
metadata:
  cli_version: ">=0.1.0"
  category: campus
  internal: true
  requires:
    bins:
      - mycqu
---

# mycqu 单体全集成 Skill (Mono Layout)

本 Skill 聚合了 `mycqu` 校园业务查询与 `cqu-reverse` 底层逆向契约。

## 业务命令速查

- 课表：`mycqu schedule list --date <YYYY-MM-DD>` 或 `mycqu schedule list --week <1-25>`
- 成绩：`mycqu score list --term current` 或 `mycqu score gpa`
- 一卡通：`mycqu card balance` 或 `mycqu card bills --start-date <YYYY-MM-DD> --end-date <YYYY-MM-DD>`
- 宿舍电费：`mycqu dorm balance --room <ROOM>`
- 认证与状态：`mycqu auth status` / `mycqu auth login`
- 接口自省：`mycqu schema [cmd]`
