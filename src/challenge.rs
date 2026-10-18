//! 不透明认证挑战 ID 的生成、持久化结构与校验。
//!
//! 挑战 ID 是待完成认证流程的状态标识：`auth login` 进入需要人工介入的 stage 时
//! 生成并与会话快照一起持久化，后续 `auth verify` **必须**提供匹配的 ID 才能继续。
//! ID 仅标识流程，不承载账号等任何身份信息。

use serde::{Deserialize, Serialize};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 挑战 ID 前缀；仅标识来源，不含任何账号信息。
const CHALLENGE_ID_PREFIX: &str = "cqu";

/// 待完成认证流程的本地最长存活时间（秒）。
///
/// 与 `rsmycqu::sso::pending::DEFAULT_PENDING_TTL_SECS` 对齐：库层面的
/// `PendingLoginState` 到期后同样会拒绝续接，本地提前到期只会让调用方
/// 更早得到明确的 `AUTH_FLOW_EXPIRED`。
pub const CHALLENGE_TTL_SECS: u64 = 300;

/// 当前 Unix 时间戳（秒）；时钟异常时退化为 0，由过期检查拒绝该挑战。
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 生成随机、不透明的挑战 ID。
///
/// 随机性来自 std 的 [`RandomState`]（进程级随机种子）叠加单调计数器与
/// 时间戳，与 `rsmycqu` 内部 CSRF key 同源；该 ID 是本地流程关联句柄而非
/// 秘密，绝不在其中编码账号等身份信息。
pub fn generate_challenge_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut first = RandomState::new().build_hasher();
    first.write_u64(now_unix());
    first.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    let hi = first.finish();

    let mut second = RandomState::new().build_hasher();
    second.write_u128(u128::from(hi));
    second.write_u64(u64::from(std::process::id()));
    let lo = second.finish();

    format!("{CHALLENGE_ID_PREFIX}-{hi:016x}{lo:016x}")
}

/// 持久化的挑战状态；不含账号、密码或任何认证秘密。
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredChallenge {
    /// 不透明挑战 ID。
    pub challenge_id: String,
    /// 创建时间（Unix 秒）。
    pub created_at: u64,
    /// 过期时间（Unix 秒）；到期后必须重新登录。
    pub expires_at: u64,
}

impl StoredChallenge {
    pub fn new(challenge_id: String, now: u64) -> Self {
        Self {
            challenge_id,
            created_at: now,
            expires_at: now.saturating_add(CHALLENGE_TTL_SECS),
        }
    }

    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }
}

/// 挑战 ID 与待完成流程的核对结果。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChallengeCheck {
    /// 提供的 ID 对应当前待完成流程。
    Current,
    /// 提供的 ID 与当前待完成流程不匹配。
    Mismatch,
    /// 本地挑战已过期。
    Expired,
    /// 没有持久化的挑战状态（流程从未记录或已被清理）。
    Missing,
}

/// 核对调用方提供的挑战 ID 是否对应当前待完成的认证流程。
///
/// 挑战 ID 是续接流程的必需标识：错误、过期或缺失都必须在读取验证码、
/// 发起网络请求之前被拒绝，而不是退化为“操作当前唯一流程”。
pub fn check_challenge(
    stored: Option<&StoredChallenge>,
    provided: &str,
    now: u64,
) -> ChallengeCheck {
    match stored {
        None => ChallengeCheck::Missing,
        Some(stored) if stored.is_expired(now) => ChallengeCheck::Expired,
        Some(stored) if stored.challenge_id == provided => ChallengeCheck::Current,
        Some(_) => ChallengeCheck::Mismatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_id_is_opaque_and_unique() {
        let ids: Vec<String> = (0..100).map(|_| generate_challenge_id()).collect();
        for id in &ids {
            assert!(id.starts_with("cqu-"), "unexpected prefix: {id}");
            let entropy = id.strip_prefix("cqu-").expect("prefix checked");
            assert_eq!(entropy.len(), 32, "expected 128-bit entropy: {id}");
            assert!(
                entropy
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                "expected lowercase hex: {id}"
            );
        }
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "challenge ids must be unique");
    }

    #[test]
    fn stored_challenge_expiry() {
        let stored = StoredChallenge::new("cqu-abc".to_string(), 1_000);
        assert_eq!(stored.expires_at, 1_000 + CHALLENGE_TTL_SECS);
        assert!(!stored.is_expired(1_000));
        assert!(stored.is_expired(1_000 + CHALLENGE_TTL_SECS));
    }

    #[test]
    fn check_challenge_matrix() {
        let stored = StoredChallenge::new("cqu-abc".to_string(), 1_000);

        assert_eq!(
            check_challenge(Some(&stored), "cqu-abc", 1_000),
            ChallengeCheck::Current
        );
        assert_eq!(
            check_challenge(Some(&stored), "cqu-wrong", 1_000),
            ChallengeCheck::Mismatch
        );
        assert_eq!(
            check_challenge(Some(&stored), "", 1_000),
            ChallengeCheck::Mismatch,
            "empty id is not a valid flow binding"
        );
        assert_eq!(
            check_challenge(Some(&stored), "cqu-abc", 1_000 + CHALLENGE_TTL_SECS),
            ChallengeCheck::Expired
        );
        assert_eq!(
            check_challenge(None, "cqu-abc", 1_000),
            ChallengeCheck::Missing
        );
    }
}
