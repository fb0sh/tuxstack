//! Protocol-neutral domain models re-exported from `tuxstack-domain`.
//!
//! Bollard/Docker DTOs never cross the adapter boundary. The compatibility
//! modules below preserve existing `crate::models::<resource>` imports while
//! every type has one authoritative definition in `tuxstack-domain`.

pub mod compose {
    pub use tuxstack_domain::compose::*;
}
pub mod container {
    pub use tuxstack_domain::container::*;
}
pub mod event {
    pub use tuxstack_domain::event::*;
}
pub mod image {
    pub use tuxstack_domain::image::*;
}
pub mod network {
    pub use tuxstack_domain::network::*;
}
pub mod options {
    pub use tuxstack_domain::options::*;
}
pub mod stats {
    pub use tuxstack_domain::stats::*;
}
pub mod system {
    pub use tuxstack_domain::system::*;
}
pub mod volume {
    pub use tuxstack_domain::volume::*;
}
pub mod volume_file;

pub use tuxstack_domain::*;
pub use volume_file::*;
