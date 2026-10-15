//! Slot roles (`Role`/`ROLES`). The six role names and their declaration
//! order are load-bearing (ROLE_LIST in predicate lint messages, the
//! `from_lower` soft-reservation order — Q12).

/// A slot role: the six lowercase names a sentence's role positions use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Actors,
    Object,
    With,
    At,
    About,
    To,
}

// The serde wire form is the lowercase string: log rows embed a role as
// `{ role: 'object' }` (in the `{ t: 'role', role }` / `{ role }`
// into-target args), so serialization is exactly `as_str`.
impl serde::Serialize for Role {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Role {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Role::from_lower(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown role '{s}'")))
    }
}

impl Role {
    /// The lowercase spelling used wherever a role appears in wire/text form
    /// (`refs` map keys, ROLE_LIST message text, the `from_lower` round-trip).
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

    /// Exact lowercase-name lookup (membership in `ROLES`; order preserved).
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

/// Declaration order is load-bearing: it feeds `ROLE_LIST` and the lint
/// messages that join it.
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
