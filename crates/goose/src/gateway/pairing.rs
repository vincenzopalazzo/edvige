use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::config::{Config, ConfigError};

use super::{PairingState, PlatformUser};

const PAIRINGS_CONFIG_KEY: &str = "gateway_pairings";
const PENDING_CODES_CONFIG_KEY: &str = "gateway_pending_codes";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPairing {
    platform: String,
    user_id: String,
    display_name: Option<String>,
    state: PairingState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPendingCode {
    code: String,
    gateway_type: String,
    expires_at: i64,
    #[serde(default, with = "super::persisted_optional_session_id")]
    session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPairing {
    pub gateway_type: String,
    pub session_id: Option<String>,
}

pub struct PairingStore {
    pairings: RwLock<HashMap<PlatformUser, PairingState>>,
    pending_codes: Mutex<()>,
}

impl PairingStore {
    pub fn new() -> anyhow::Result<Self> {
        let pairings = Self::load_pairings_from_config()?;
        Ok(Self {
            pairings: RwLock::new(pairings),
            pending_codes: Mutex::new(()),
        })
    }

    fn load_pairings_from_config() -> anyhow::Result<HashMap<PlatformUser, PairingState>> {
        let config = Config::global();
        let entries: Vec<StoredPairing> = match config.get_param(PAIRINGS_CONFIG_KEY) {
            Ok(entries) => entries,
            Err(ConfigError::NotFound(_)) => Vec::new(),
            Err(error) => return Err(anyhow::anyhow!("failed to load gateway pairings: {error}")),
        };
        let mut map = HashMap::new();
        for mut entry in entries {
            normalize_pairing_state(&mut entry.state)?;
            let user = PlatformUser {
                platform: entry.platform,
                user_id: entry.user_id,
                display_name: entry.display_name,
            };
            map.insert(user, entry.state);
        }
        Ok(map)
    }

    fn save_pairings_to_config(
        pairings: &HashMap<PlatformUser, PairingState>,
    ) -> anyhow::Result<()> {
        let entries: Vec<StoredPairing> = pairings
            .iter()
            .map(|(user, state)| StoredPairing {
                platform: user.platform.clone(),
                user_id: user.user_id.clone(),
                display_name: user.display_name.clone(),
                state: state.clone(),
            })
            .collect();
        Config::global()
            .set_param(PAIRINGS_CONFIG_KEY, &entries)
            .map_err(|e| anyhow::anyhow!("failed to save gateway pairings: {}", e))
    }

    fn save_pairings_or_rollback(
        pairings: &mut HashMap<PlatformUser, PairingState>,
        previous: HashMap<PlatformUser, PairingState>,
    ) -> anyhow::Result<()> {
        if let Err(error) = Self::save_pairings_to_config(pairings) {
            *pairings = previous;
            return Err(error);
        }
        Ok(())
    }

    fn load_pending_codes() -> anyhow::Result<Vec<StoredPendingCode>> {
        match Config::global().get_param(PENDING_CODES_CONFIG_KEY) {
            Ok(codes) => Ok(codes),
            Err(ConfigError::NotFound(_)) => Ok(Vec::new()),
            Err(error) => Err(anyhow::anyhow!(
                "failed to load pending gateway codes: {error}"
            )),
        }
    }

    fn save_pending_codes(codes: &[StoredPendingCode]) -> anyhow::Result<()> {
        Config::global()
            .set_param(PENDING_CODES_CONFIG_KEY, codes)
            .map_err(|e| anyhow::anyhow!("failed to save pending codes: {}", e))
    }

    pub async fn get(&self, user: &PlatformUser) -> anyhow::Result<PairingState> {
        let pairings = self.pairings.read().await;
        Ok(pairings
            .get(user)
            .cloned()
            .unwrap_or(PairingState::Unpaired))
    }

    pub async fn set(&self, user: &PlatformUser, state: PairingState) -> anyhow::Result<()> {
        let mut pairings = self.pairings.write().await;
        let previous = pairings.clone();
        let mut state = state;
        normalize_pairing_state(&mut state)?;
        pairings.insert(user.clone(), state);
        Self::save_pairings_or_rollback(&mut pairings, previous)
    }

    pub async fn pair_with_session(
        &self,
        user: &PlatformUser,
        session_id: &str,
        paired_at: i64,
        exclusive_session: bool,
    ) -> anyhow::Result<()> {
        let mut pairings = self.pairings.write().await;
        if exclusive_session && session_has_other_pairing(&pairings, user, session_id) {
            anyhow::bail!(
                "session '{}' is already paired to another {} group",
                session_id,
                user.platform
            );
        }
        let previous = pairings.clone();

        if session_id.is_empty() {
            anyhow::bail!("cannot pair an empty session id");
        }

        update_pairing_with_session(
            &mut pairings,
            user,
            session_id,
            paired_at,
            exclusive_session,
        );
        Self::save_pairings_or_rollback(&mut pairings, previous)
    }

    pub async fn authorize_and_activate_session(
        &self,
        user: &PlatformUser,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let mut pairings = self.pairings.write().await;
        if session_id.is_empty() {
            anyhow::bail!("cannot authorize an empty session id");
        }
        if session_has_other_pairing(&pairings, user, session_id) {
            anyhow::bail!(
                "session '{}' is already authorized for another {} group",
                session_id,
                user.platform
            );
        }
        let previous = pairings.clone();
        authorize_and_activate_session(&mut pairings, user, session_id)?;
        Self::save_pairings_or_rollback(&mut pairings, previous)
    }

    pub async fn activate_session(
        &self,
        user: &PlatformUser,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let mut pairings = self.pairings.write().await;
        let previous = pairings.clone();
        activate_session(&mut pairings, user, session_id)?;
        Self::save_pairings_or_rollback(&mut pairings, previous)
    }

    pub async fn authorized_sessions(&self, user: &PlatformUser) -> anyhow::Result<Vec<String>> {
        let pairings = self.pairings.read().await;
        match pairings.get(user) {
            Some(PairingState::Paired { session_ids, .. }) => Ok(session_ids.clone()),
            _ => anyhow::bail!("Sonar group is not authorized"),
        }
    }

    pub async fn remove(&self, user: &PlatformUser) -> anyhow::Result<()> {
        let mut pairings = self.pairings.write().await;
        let previous = pairings.clone();
        pairings.remove(user);
        Self::save_pairings_or_rollback(&mut pairings, previous)
    }

    pub async fn store_pending_code(
        &self,
        code: &str,
        gateway_type: &str,
        expires_at: i64,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let _pending_codes = self.pending_codes.lock().await;
        let mut codes = Self::load_pending_codes()?;
        codes.retain(|c| c.code != code);
        codes.push(StoredPendingCode {
            code: code.to_string(),
            gateway_type: gateway_type.to_string(),
            expires_at,
            session_id: session_id.map(str::to_string),
        });
        Self::save_pending_codes(&codes)
    }

    pub async fn consume_pending_code(&self, code: &str) -> anyhow::Result<Option<PendingPairing>> {
        let _pending_codes = self.pending_codes.lock().await;
        let mut codes = Self::load_pending_codes()?;
        let pos = codes.iter().position(|c| c.code == code);
        let Some(pos) = pos else {
            return Ok(None);
        };

        let entry = codes.remove(pos);
        Self::save_pending_codes(&codes)?;

        let now = chrono::Utc::now().timestamp();
        if now > entry.expires_at {
            return Ok(None);
        }

        Ok(Some(PendingPairing {
            gateway_type: entry.gateway_type,
            session_id: entry.session_id,
        }))
    }

    pub fn generate_code() -> String {
        use rand::RngExt;
        let chars: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        let mut rng = rand::rng();
        (0..6)
            .map(|_| chars[rng.random_range(0..chars.len())] as char)
            .collect()
    }

    pub async fn remove_all_for_platform(&self, platform: &str) -> anyhow::Result<usize> {
        let mut pairings = self.pairings.write().await;
        let previous = pairings.clone();
        let before = pairings.len();
        pairings.retain(|user, _| user.platform != platform);
        let removed = before - pairings.len();
        Self::save_pairings_or_rollback(&mut pairings, previous)?;
        Ok(removed)
    }

    pub async fn list_paired_users(
        &self,
        gateway_type: &str,
    ) -> anyhow::Result<Vec<(PlatformUser, String, i64)>> {
        let pairings = self.pairings.read().await;
        let mut result = Vec::new();
        for (user, state) in pairings.iter() {
            if user.platform == gateway_type {
                if let PairingState::Paired {
                    session_id,
                    paired_at,
                    ..
                } = state
                {
                    result.push((user.clone(), session_id.clone(), *paired_at));
                }
            }
        }
        Ok(result)
    }
}

fn session_has_other_pairing(
    pairings: &HashMap<PlatformUser, PairingState>,
    user: &PlatformUser,
    session_id: &str,
) -> bool {
    pairings.iter().any(|(paired_user, state)| {
        paired_user != user
            && paired_user.platform == user.platform
            && pairing_contains_session(state, session_id)
    })
}

fn pairing_contains_session(state: &PairingState, session_id: &str) -> bool {
    matches!(
        state,
        PairingState::Paired {
            session_id: active_session,
            session_ids,
            ..
        } if active_session == session_id || session_ids.iter().any(|authorized| authorized == session_id)
    )
}

fn update_pairing_with_session(
    pairings: &mut HashMap<PlatformUser, PairingState>,
    user: &PlatformUser,
    session_id: &str,
    paired_at: i64,
    retain_authorized_sessions: bool,
) {
    if retain_authorized_sessions {
        let state = pairings
            .entry(user.clone())
            .or_insert(PairingState::Unpaired);
        match state {
            PairingState::Paired {
                session_id: active_session_id,
                session_ids,
                ..
            } => {
                if !session_ids
                    .iter()
                    .any(|authorized| authorized == session_id)
                {
                    session_ids.push(session_id.to_string());
                }
                *active_session_id = session_id.to_string();
            }
            _ => {
                *state = PairingState::Paired {
                    session_id: session_id.to_string(),
                    paired_at,
                    session_ids: vec![session_id.to_string()],
                };
            }
        }
    } else {
        pairings.insert(
            user.clone(),
            PairingState::Paired {
                session_id: session_id.to_string(),
                paired_at,
                session_ids: vec![session_id.to_string()],
            },
        );
    }
}

fn normalize_pairing_state(state: &mut PairingState) -> anyhow::Result<()> {
    let PairingState::Paired {
        session_id,
        session_ids,
        ..
    } = state
    else {
        return Ok(());
    };

    if session_id.is_empty() {
        anyhow::bail!("paired gateway state has an empty active session id");
    }

    let mut seen = std::collections::HashSet::new();
    session_ids.retain(|authorized| !authorized.is_empty() && seen.insert(authorized.clone()));
    if !session_ids
        .iter()
        .any(|authorized| authorized == session_id)
    {
        session_ids.insert(0, session_id.clone());
    }
    Ok(())
}

fn authorize_and_activate_session(
    pairings: &mut HashMap<PlatformUser, PairingState>,
    user: &PlatformUser,
    session_id: &str,
) -> anyhow::Result<()> {
    let state = pairings
        .get_mut(user)
        .ok_or_else(|| anyhow::anyhow!("Sonar group is not authorized"))?;
    let PairingState::Paired {
        session_id: active_session,
        session_ids,
        ..
    } = state
    else {
        anyhow::bail!("Sonar group is not authorized");
    };
    if !session_ids
        .iter()
        .any(|authorized| authorized == session_id)
    {
        session_ids.push(session_id.to_string());
    }
    *active_session = session_id.to_string();
    Ok(())
}

fn activate_session(
    pairings: &mut HashMap<PlatformUser, PairingState>,
    user: &PlatformUser,
    session_id: &str,
) -> anyhow::Result<()> {
    let state = pairings
        .get_mut(user)
        .ok_or_else(|| anyhow::anyhow!("Sonar group is not authorized"))?;
    let PairingState::Paired {
        session_id: active_session,
        session_ids,
        ..
    } = state
    else {
        anyhow::bail!("Sonar group is not authorized");
    };
    if !session_ids
        .iter()
        .any(|authorized| authorized == session_id)
    {
        anyhow::bail!(
            "session '{}' is not authorized for this Sonar group",
            session_id
        );
    }
    *active_session = session_id.to_string();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_generation() {
        let code = PairingStore::generate_code();
        assert_eq!(code.len(), 6);
        assert!(code
            .chars()
            .all(|c| "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(c)));
    }

    #[test]
    fn legacy_pending_codes_deserialize_without_session() {
        let code: StoredPendingCode = serde_json::from_value(serde_json::json!({
            "code": "ABC234",
            "gateway_type": "telegram",
            "expires_at": 42
        }))
        .unwrap();

        assert_eq!(code.session_id, None);
    }

    #[test]
    fn pending_code_preserves_underscore_session_id() {
        let code = StoredPendingCode {
            code: "ABC234".into(),
            gateway_type: "sonar".into(),
            expires_at: 42,
            session_id: Some("20260712_50".into()),
        };

        let yaml = serde_yaml::to_string(&code).unwrap();
        let reloaded: StoredPendingCode = serde_yaml::from_str(&yaml).unwrap();
        let legacy: StoredPendingCode = serde_yaml::from_str(
            "code: ABC234\ngateway_type: sonar\nexpires_at: 42\nsession_id: 20260712_50\n",
        )
        .unwrap();

        assert_eq!(reloaded.session_id.as_deref(), Some("20260712_50"));
        assert_eq!(legacy.session_id.as_deref(), Some("20260712_50"));
    }

    #[test]
    fn sonar_session_pairing_is_exclusive_between_groups() {
        let first = PlatformUser {
            platform: "sonar".into(),
            user_id: "group-a".into(),
            display_name: None,
        };
        let second = PlatformUser {
            platform: "sonar".into(),
            user_id: "group-b".into(),
            display_name: None,
        };
        let pairings = HashMap::from([(
            first.clone(),
            PairingState::Paired {
                session_id: "active".into(),
                paired_at: 1,
                session_ids: vec!["session".into(), "active".into()],
            },
        )]);

        assert!(!session_has_other_pairing(&pairings, &first, "active"));
        assert!(session_has_other_pairing(&pairings, &second, "session"));
    }

    #[test]
    fn legacy_pairing_adds_active_session_to_authorized_sessions() {
        let mut pairing: PairingState =
            serde_yaml::from_str("state: paired\nsession_id: 20260712_50\npaired_at: 42\n")
                .unwrap();

        normalize_pairing_state(&mut pairing).unwrap();

        assert!(matches!(
            pairing,
            PairingState::Paired {
                session_id,
                paired_at: 42,
                session_ids,
            } if session_id == "20260712_50" && session_ids == ["20260712_50"]
        ));
    }

    #[test]
    fn rejects_empty_active_session_id() {
        let mut pairing = PairingState::Paired {
            session_id: String::new(),
            paired_at: 42,
            session_ids: Vec::new(),
        };

        assert!(normalize_pairing_state(&mut pairing).is_err());
    }

    #[test]
    fn persisted_pairing_preserves_underscore_session_ids() {
        let pairing = PairingState::Paired {
            session_id: "20260712_50".into(),
            paired_at: 42,
            session_ids: vec!["20260712_49".into(), "20260712_50".into()],
        };

        let yaml = serde_yaml::to_string(&pairing).unwrap();
        let reloaded: PairingState = serde_yaml::from_str(&yaml).unwrap();

        assert!(yaml.contains("goose-session-id:20260712_50"));
        assert!(matches!(
            reloaded,
            PairingState::Paired {
                session_id,
                paired_at: 42,
                session_ids,
            } if session_id == "20260712_50"
                && session_ids == ["20260712_49", "20260712_50"]
        ));
    }

    #[test]
    fn rejects_numeric_session_ids_outside_the_legacy_date_sequence_schema() {
        let invalid: Result<PairingState, _> =
            serde_yaml::from_str("state: paired\nsession_id: 1234567890\npaired_at: 42\n");

        assert!(invalid.is_err());
    }

    #[test]
    fn switching_sessions_preserves_authorization_time_and_scope() {
        let user = PlatformUser {
            platform: "sonar".into(),
            user_id: "group-a".into(),
            display_name: None,
        };
        let mut pairings = HashMap::from([(
            user.clone(),
            PairingState::Paired {
                session_id: "first".into(),
                paired_at: 42,
                session_ids: vec!["first".into()],
            },
        )]);

        authorize_and_activate_session(&mut pairings, &user, "second").unwrap();
        activate_session(&mut pairings, &user, "first").unwrap();

        assert!(matches!(
            pairings.get(&user),
            Some(PairingState::Paired {
                session_id,
                paired_at: 42,
                session_ids,
            }) if session_id == "first" && session_ids == &["first", "second"]
        ));
        assert!(activate_session(&mut pairings, &user, "other").is_err());
    }

    #[test]
    fn repeated_exclusive_pairing_retains_owned_sessions_and_original_authorization_time() {
        let user = PlatformUser {
            platform: "sonar".into(),
            user_id: "group-a".into(),
            display_name: None,
        };
        let mut pairings = HashMap::new();

        update_pairing_with_session(&mut pairings, &user, "first", 42, true);
        update_pairing_with_session(&mut pairings, &user, "second", 99, true);
        update_pairing_with_session(&mut pairings, &user, "second", 100, true);

        assert!(matches!(
            pairings.get(&user),
            Some(PairingState::Paired {
                session_id,
                paired_at: 42,
                session_ids,
            }) if session_id == "second" && session_ids == &["first", "second"]
        ));
    }
}
