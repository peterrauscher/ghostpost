//! Server-truth free_beta entitlement policy (no public grant HTTP).
use crate::repository::entitlements::{self, EntitlementGrant};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub const FREE_BETA_PLATFORM_LIMIT: i32 = 2;
pub const FREE_BETA_PRODUCT: &str = "free_beta";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitlementPolicy {
    FreeBeta,
}

impl EntitlementPolicy {
    pub fn from_env() -> Self {
        match std::env::var("ENTITLEMENT_POLICY")
            .unwrap_or_else(|_| "free_beta".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "free_beta" | "" => Self::FreeBeta,
            other => {
                tracing::warn!(policy = %other, "unknown ENTITLEMENT_POLICY; defaulting to free_beta");
                Self::FreeBeta
            }
        }
    }

    pub fn product_key(self) -> &'static str {
        match self {
            Self::FreeBeta => FREE_BETA_PRODUCT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EffectiveEntitlement {
    pub grant_id: Uuid,
    pub product_id: String,
    pub status: String,
    pub valid_from: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub review_access: bool,
    /// `None` means unlimited rescans (free_beta).
    pub rescans_remaining: Option<i32>,
    pub platform_limit: i32,
    pub scan_scope_id: Option<Uuid>,
}

pub struct EntitlementService;

impl EntitlementService {
    pub fn policy() -> EntitlementPolicy {
        EntitlementPolicy::from_env()
    }

    /// Insert free_beta grant inside the signup transaction (promotion source).
    pub async fn seed_free_beta_tx(
        tx: &mut Transaction<'_, Postgres>,
        tenant_id: Uuid,
        user_id: Uuid,
    ) -> Result<EntitlementGrant, sqlx::Error> {
        entitlements::grant_free_beta_promotion_tx(tx, tenant_id, user_id).await
    }

    pub async fn resolve(
        pool: &PgPool,
        tenant_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<EffectiveEntitlement>, sqlx::Error> {
        let policy = Self::policy();
        let Some(grant) =
            entitlements::get_active_grant(pool, tenant_id, user_id, policy.product_key()).await?
        else {
            return Ok(None);
        };
        Self::to_effective(pool, &grant).await.map(Some)
    }

    pub async fn resolve_tx(
        tx: &mut Transaction<'_, Postgres>,
        tenant_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<EffectiveEntitlement>, sqlx::Error> {
        let policy = Self::policy();
        let Some(grant) =
            entitlements::get_active_grant_tx(tx, tenant_id, user_id, policy.product_key()).await?
        else {
            return Ok(None);
        };
        Self::to_effective_tx(tx, &grant).await.map(Some)
    }

    pub async fn require_review_access(
        pool: &PgPool,
        tenant_id: Uuid,
        user_id: Uuid,
    ) -> Result<EffectiveEntitlement, EntitlementDeny> {
        match Self::resolve(pool, tenant_id, user_id).await {
            Ok(Some(e)) if e.review_access => Ok(e),
            Ok(Some(_)) | Ok(None) => Err(EntitlementDeny::Required),
            Err(err) => Err(EntitlementDeny::Db(err)),
        }
    }

    /// free_beta: unlimited — always Ok and does not insert usage.
    /// Counted SKUs (future): decrement via entitlement_usages.
    pub async fn consume_rescan(
        tx: &mut Transaction<'_, Postgres>,
        entitlement: &EffectiveEntitlement,
        tenant_id: Uuid,
        user_id: Uuid,
        scan_id: Uuid,
    ) -> Result<(), EntitlementDeny> {
        match entitlement.rescans_remaining {
            None => Ok(()), // unlimited free_beta
            Some(0) => Err(EntitlementDeny::Required),
            Some(_) => {
                entitlements::insert_usage_tx(
                    tx,
                    tenant_id,
                    entitlement.grant_id,
                    user_id,
                    scan_id,
                    "rescan",
                )
                .await
                .map_err(EntitlementDeny::Db)?;
                Ok(())
            }
        }
    }

    async fn to_effective(
        pool: &PgPool,
        grant: &EntitlementGrant,
    ) -> Result<EffectiveEntitlement, sqlx::Error> {
        let used = if grant.rescan_limit.is_some() {
            entitlements::count_usages(pool, grant.tenant_id, grant.id, "rescan").await?
        } else {
            0
        };
        Ok(map_effective(grant, used))
    }

    async fn to_effective_tx(
        tx: &mut Transaction<'_, Postgres>,
        grant: &EntitlementGrant,
    ) -> Result<EffectiveEntitlement, sqlx::Error> {
        let used = if grant.rescan_limit.is_some() {
            entitlements::count_usages_tx(tx, grant.tenant_id, grant.id, "rescan").await?
        } else {
            0
        };
        Ok(map_effective(grant, used))
    }
}

fn map_effective(grant: &EntitlementGrant, used: i64) -> EffectiveEntitlement {
    let platform_limit = grant
        .platform_limit
        .unwrap_or(FREE_BETA_PLATFORM_LIMIT)
        .max(1);
    let rescans_remaining = grant.rescan_limit.map(|lim| (lim as i64 - used).max(0) as i32);
    let active = grant.revoked_at.is_none()
        && grant
            .valid_until
            .map(|u| u > Utc::now())
            .unwrap_or(true);
    EffectiveEntitlement {
        grant_id: grant.id,
        product_id: grant.product_key.clone(),
        status: if active { "active".into() } else { "inactive".into() },
        valid_from: grant.valid_from,
        expires_at: grant.valid_until,
        review_access: grant.review_access && active,
        rescans_remaining,
        platform_limit,
        scan_scope_id: grant.scan_scope_id,
    }
}

#[derive(Debug)]
pub enum EntitlementDeny {
    Required,
    Db(sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_beta_policy_default() {
        std::env::remove_var("ENTITLEMENT_POLICY");
        assert_eq!(EntitlementPolicy::from_env(), EntitlementPolicy::FreeBeta);
        assert_eq!(EntitlementPolicy::FreeBeta.product_key(), "free_beta");
    }

    #[test]
    fn map_unlimited_rescan() {
        let grant = EntitlementGrant {
            tenant_id: Uuid::nil(),
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            product_key: "free_beta".into(),
            grant_source: "promotion".into(),
            review_access: true,
            rescan_limit: None,
            platform_limit: Some(2),
            scan_scope_id: None,
            valid_from: Utc::now(),
            valid_until: None,
            revoked_at: None,
            created_at: Utc::now(),
        };
        let e = map_effective(&grant, 99);
        assert!(e.review_access);
        assert_eq!(e.platform_limit, 2);
        assert_eq!(e.rescans_remaining, None);
        assert_eq!(e.product_id, "free_beta");
    }
}
