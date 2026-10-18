//! 命令共用的服务授权编排：在状态锁内完成“读取会话 → 申请服务权限 → 持久化”。

use rsmycqu::card::access_card;
use rsmycqu::mycqu::access_mycqu;
use rsmycqu::session::{Client, Session};

use crate::client::map_api_error;
use crate::store::{self, StateStore};

/// 需要单独申请访问权限的校园服务。
///
/// 每个服务由统一身份认证换取自己的凭据，失败原因与权限范围互不相同。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Service {
    /// 教务网（`my.cqu.edu.cn`）
    Mycqu,
    /// 校园卡与宿舍（`card.cqu.edu.cn`）
    Card,
}

impl Service {
    /// 报告错误时使用的服务名。
    fn label(self) -> &'static str {
        match self {
            Service::Mycqu => "mycqu",
            Service::Card => "card",
        }
    }
}

/// 在状态锁内完成“读取会话 → 申请服务访问权限 → 持久化”，返回可直接查询的会话。
///
/// # 锁的作用域
/// 锁只覆盖读取-修改-保存：调用方拿到会话后即可在锁外发起业务查询，
/// 并行调用的多个进程不会因为查询而互相排队。
///
/// # 为什么授权请求留在锁内
/// 授权会改写 Cookie、服务凭据与瑞数状态；若在锁外更新再落盘，就会重新
/// 引入“过期快照覆盖新状态”的问题。临界区以一次授权请求为界，不会长期占锁。
///
/// 查询本身会修改会话的命令（如 `dorm balance`）需要把查询也纳入临界区，
/// 因此不复用本函数，而是显式持有 [`crate::store::StateLock`]。
pub async fn authorized_session(
    store: &StateStore,
    client: &Client,
    service: Service,
) -> Result<Session, i32> {
    let _lock = store::lock_or_report(store)?;
    let mut session = store::load_session_or_report(store)?;

    let context = format!("Failed to access {} service", service.label());
    let access_error = match service {
        Service::Mycqu => access_mycqu(client, &mut session)
            .await
            .err()
            .map(|e| map_api_error(&context, &e)),
        Service::Card => access_card(client, &mut session)
            .await
            .err()
            .map(|e| map_api_error(&context, &e)),
    };
    if let Some(mapped) = access_error {
        mapped.report();
        return Err(mapped.exit_code);
    }

    store::save_session_or_report(store, &session)?;
    Ok(session)
}
