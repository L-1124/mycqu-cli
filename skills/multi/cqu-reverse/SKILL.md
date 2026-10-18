---
name: cqu-reverse
description: 重庆大学 (CQU) 统一身份认证 (SSO) 与 mycqu 服务逆向工程技能。涵盖 CQU 瑞数 6 (Ruishu) 防护逆向、Cookie T (CtLgrmkSWHBZP) 纯算与混合生成流水线、86 字符真实字符表、51 组密钥提取算法、TLS 指纹绕过、以及端到端接口验证。
metadata:
  category: reverse-engineering
---

# CQU 逆向工程指南 (cqu-reverse)

本技能提供针对重庆大学 (CQU) 统一身份认证 (SSO, `sso.cqu.edu.cn`) 与数字重大学 (`my.cqu.edu.cn`) 防护体系的完整逆向知识、算法流水线、环境补丁与验证脚本。

## 适用场景

1. **CQU SSO 瑞数防护逆向**：需要绕过或模拟 CQU SSO 页面的瑞数 6 (Ruishu 6) 动态防御，生成合法的反爬凭据。
2. **凭据生成与接口调用**：计算客户端特征 Cookie T (`CtLgrmkSWHBZP`)，配合服务端 Cookie O (`CtLgrmkSWHBZO`) 调用受保护接口。
3. **接口故障排查**：诊断请求返回 HTTP 412 (瑞数拦截)、HTTP 400 (明文结构错误) 或网络层 TLS 指纹被 WAF 识别等问题。
4. **Rust 客户端集成**：将纯算加密链与会话提取逻辑集成到 `rsmycqu` 库中，解决 reqwest TLS 握手特征阻断。

## 核心算法流水线

CQU SSO 的安全 Cookie 计算分为五步：

```text
[1. 页面抓取] -> 提取 $_ts.cd 与 $_ts.nsd
[2. 密钥解析] -> 86字符表解码 + 异或掩码(s[0]^51) -> 51组 keys
[3. 明文组装] -> [CRC32(4)] + [4, 8, t1, t2] + [48, keys2] + [0x80, 160, basearr]
[4. 分组加密] -> AES-128-CBC(站点常量 keys16, 随机 16B IV)
[5. 特制编码] -> 64字符表编码(余数位标准左对齐) + 前缀 'a' -> Cookie T (343 字符)
```

## 关键技术常数

- **真实 86 字符表**：
  `qrcklmDoExthWJiHAp1sVYKU3RFMQw8IGfPO92bvLNj-7zXBaSnu0TC6gy_4Ze5d{}|~ !#$%()*+,.;=?@[]^`
  （关键特征：`-` 位于下标 43，`.` 位于尾部下标 78，与通用公开实现相反）。
- **Base64 64 字符表**：前 64 字符（`qrcklmDoExthWJiHAp1sVYKU3RFMQw8IGfPO92bvLNj-7zXBaSnu0TC6gy_4Ze5d`）。
- **站点加密常量 `keys[16]`**：`0aaf65014a424d1542f8e927e24adfc4`（全站点固定）。
- **密钥提取掩码**：`MASK = [51, 2, 0x36, 0x34, 2, 0x36, 0x34, 48]`（首字节为 51，非 45）。
- **明文固定长度**：225 字节。包含 160 字节的 `basearr` 实例数据。

## 生产方案：全纯算

不再需要 JS 运行时：`cd`/`nsd` 取自**登录页挑战**，`mainjs` 取一次（部署级固定），其余全部纯算。

1. **引导（每部署一次）**：取登录页里 `<script src=...>` 指向的 mainjs 源码，交给 `Session::set_mainjs`。
2. **每会话自动**：库在解析登录页时记住页面挑战（`cd`/`nsd`）；此后命中 412 会自动算出 basearr 与 Cookie T 并重发。

```rust
session.set_mainjs(&mainjs);              // 部署级
// 登录页由 sso::login 拉取即自动采集挑战；长会话可手动刷新：
session.set_ruishu_challenge_from_page(&fresh_page_html);
```
