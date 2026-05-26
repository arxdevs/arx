use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, self.0)
            }
        }
    };
}

id_type!(UserId, "usr");
id_type!(WorkspaceId, "ws");
id_type!(MemberId, "mem");
id_type!(ProjectId, "prj");
id_type!(EnvironmentId, "env");
id_type!(ServiceId, "svc");
id_type!(DeploymentId, "dep");
id_type!(VariableId, "var");
id_type!(DomainId, "dom");
id_type!(WebhookEventId, "wh");
