use serde_json::Value;

/// 解析命令目录 JSON。
///
/// 解析失败或结构不符必须返回错误，由调用方显式报告；
/// 不允许静默回退为空目录——那会让 Agent 看到合法 JSON 却找不到任何命令。
pub fn parse_catalog(raw: &str) -> Result<Value, String> {
    let commands: Value =
        serde_json::from_str(raw).map_err(|e| format!("malformed schema catalog JSON: {e}"))?;
    if !commands.is_array() {
        return Err("schema catalog must be a JSON array of command definitions".to_string());
    }
    Ok(commands)
}

/// 返回命令目录；目录损坏时返回错误而不是空数组。
pub fn schema_catalog() -> Result<Value, String> {
    let commands = parse_catalog(include_str!("catalog.json"))?;
    Ok(serde_json::json!({ "commands": commands }))
}
