//! Identifier types. Numeric ids are mint-order addresses the kernel
//! allocates; name ids are the one global content namespace
//! (spec.md §4.1, language.md §1).

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! u64_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

macro_rules! name_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name(s.to_string())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                $name(s)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

u64_id!(
    /// A live card instance; mint order.
    EntityId
);
u64_id!(
    /// A zone address; declared zones and kernel-minted zones share the space.
    ZoneId
);
u64_id!(
    /// One open situation instance.
    SituationId
);

name_id!(
    /// A card declaration name (the entity/prototype namespace, §2.4).
    CardId
);
name_id!(
    /// A verb name.
    VerbId
);
name_id!(
    /// A globally unique recipe address spelled `<verb>.<recipe>`.
    RecipeId
);
name_id!(
    /// A deck name.
    DeckId
);
name_id!(
    /// An ending id.
    EndingId
);
name_id!(
    /// A recipe slot name.
    SlotName
);
name_id!(
    /// A trigger rule address, globally unique across every book site.
    TriggerRuleId
);
