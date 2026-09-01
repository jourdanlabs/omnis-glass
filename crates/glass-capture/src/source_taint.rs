//! Source-taint provenance resolution — screencast is never provenance.

use crate::CaptureKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaintVerdict {
    Accepted,
    Rejected(String),
}

/// Resolve whether a capture may be used as URL provenance for downstream taint.
pub fn resolve(capture_kind: CaptureKind) -> TaintVerdict {
    if capture_kind == CaptureKind::Screencast {
        return TaintVerdict::Rejected(
            "capture_kind screencast is observability-only, not source provenance".into(),
        );
    }
    TaintVerdict::Accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CaptureKind;

    #[test]
    fn screencast_rejected_for_provenance() {
        assert!(matches!(
            resolve(CaptureKind::Screencast),
            TaintVerdict::Rejected(_)
        ));
    }

    #[test]
    fn live_accepted_for_provenance() {
        assert_eq!(resolve(CaptureKind::Live), TaintVerdict::Accepted);
    }
}
