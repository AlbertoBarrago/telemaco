pub mod context;
mod fork_virtual_url;
pub mod lifecycle;
pub mod page;
#[cfg(feature = "render")]
pub mod pdf;
pub mod profiles;

pub use context::BrowserContext;
pub use lifecycle::{LifecycleState, WaitUntil};
pub use telemaco_js::HTML_TO_MARKDOWN_JS;
#[cfg(feature = "render")]
pub use telemaco_js::{
    validate_capture_region, AnimationSample, AnimationSampleMode, AnimationSampleTime,
    CaptureError, CaptureRegion,
};
pub use page::{NetworkEvent, Page, PageError};
#[cfg(feature = "render")]
pub use pdf::{RasterPdfError, RasterPdfOptions, RasterPdfPageRange};
// Re-exported so the embeddable `telemaco` crate (which depends on telemaco-browser,
// not telemaco-js) can surface the interception channel types.
pub use telemaco_js::ops::{InterceptResolution, InterceptedRequest};
