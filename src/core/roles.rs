//! Slot roles (`Role`/`ROLES`). The role names and their declaration
//! order are load-bearing (ROLE_LIST in predicate lint messages, the
//! `from_lower` soft-reservation order — Q12). `Held` is the seventh role
//! (spec §4.3): a verb's held slot (§6.1) binds through a HOLD step
//! (§6.7), never in a clause — clauses carry no held position, so every
//! clause-shaped read of `refs[Held]` sees the empty binding.

/// A slot role: the lowercase names a role position uses — the six clause
/// roles plus `held` (§4.3, §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Actors,
    Object,
    With,
    At,
    About,
    To,
    Held,
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
        Role::from_lower(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown role '{s}'")))
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
            Role::Held => "held",
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
            "held" => Some(Role::Held),
            _ => None,
        }
    }
}

/// Declaration order is load-bearing: it feeds `ROLE_LIST` and the lint
/// messages that join it. The six clause roles come first (their order is
/// the clause-refs and bindings-map order); `held` joins last — it is never
/// a clause position (§4.3).
pub const ROLES: [Role; 7] =
    [Role::Actors, Role::Object, Role::With, Role::At, Role::About, Role::To, Role::Held];

/// The role names in [`ROLES`] order — the spelling every joined role list
/// uses (the predicate lint's ROLE_LIST, the script lint's known-role list).
#[must_use]
pub fn role_names() -> Vec<&'static str> {
    ROLES.iter().map(|r| r.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_round_trip_and_keep_declaration_order() {
        let names: Vec<&str> = ROLES.iter().map(|r| r.as_str()).collect();
        assert_eq!(names, ["actors", "object", "with", "at", "about", "to", "held"]);
        for r in ROLES {
            assert_eq!(Role::from_lower(r.as_str()), Some(r));
        }
        assert_eq!(Role::from_lower("ACTORS"), None); // only exact lowercase
        assert_eq!(Role::from_lower("objects"), None);
    }
}
