//! Slot roles — sunk here from TS `sentence.ts` (plan §2.2: `Role/ROLES`
//! 下沉 core/roles.rs, CORE-02). Zero behavior change: the six role names
//! and their declaration order are load-bearing (ROLE_LIST in predicate lint
//! messages, `asRole` soft-reservation order — Q12).

/// ts: src/core/sentence.ts:6 — `type Role = 'actors' | 'object' | 'with' |
/// 'at' | 'about' | 'to'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Actors,
    Object,
    With,
    At,
    About,
    To,
}

impl Role {
    /// The lowercase spelling every TS site uses (`refs: Record<Role, Ref[]>`
    /// keys, ROLE_LIST message text, `asRole` round-trip).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Actors => "actors",
            Role::Object => "object",
            Role::With => "with",
            Role::At => "at",
            Role::About => "about",
            Role::To => "to",
        }
    }

    /// Exact lowercase-name lookup (the TS `ROLES.includes(lower)` membership
    /// after an explicit `ROLES` list check; order preserved).
    #[must_use]
    pub fn from_lower(s: &str) -> Option<Role> {
        match s {
            "actors" => Some(Role::Actors),
            "object" => Some(Role::Object),
            "with" => Some(Role::With),
            "at" => Some(Role::At),
            "about" => Some(Role::About),
            "to" => Some(Role::To),
            _ => None,
        }
    }
}

/// ts: src/core/sentence.ts:8 — declaration order is load-bearing: it feeds
/// `ROLE_LIST` (`predicate.ts:518`) and the lint messages that join it.
pub const ROLES: [Role; 6] = [
    Role::Actors,
    Role::Object,
    Role::With,
    Role::At,
    Role::About,
    Role::To,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_round_trip_and_keep_ts_order() {
        let names: Vec<&str> = ROLES.iter().map(|r| r.as_str()).collect();
        assert_eq!(names, ["actors", "object", "with", "at", "about", "to"]);
        for r in ROLES {
            assert_eq!(Role::from_lower(r.as_str()), Some(r));
        }
        assert_eq!(Role::from_lower("ACTORS"), None); // only exact lowercase
        assert_eq!(Role::from_lower("objects"), None);
    }
}
