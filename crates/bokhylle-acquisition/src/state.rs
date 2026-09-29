use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AcquisitionStatus {
    Requested,
    Searching,
    Evaluating,
    Queued,
    Downloading,
    Downloaded,
    Inspecting,
    Identified,
    Importing,
    Ready,
    NoReleaseFound,
    NeedsSelection,
    DownloadFailed,
    ImportFailed,
    NeedsReview,
    Cancelled,
}

impl AcquisitionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "REQUESTED",
            Self::Searching => "SEARCHING",
            Self::Evaluating => "EVALUATING",
            Self::Queued => "QUEUED",
            Self::Downloading => "DOWNLOADING",
            Self::Downloaded => "DOWNLOADED",
            Self::Inspecting => "INSPECTING",
            Self::Identified => "IDENTIFIED",
            Self::Importing => "IMPORTING",
            Self::Ready => "READY",
            Self::NoReleaseFound => "NO_RELEASE_FOUND",
            Self::NeedsSelection => "NEEDS_SELECTION",
            Self::DownloadFailed => "DOWNLOAD_FAILED",
            Self::ImportFailed => "IMPORT_FAILED",
            Self::NeedsReview => "NEEDS_REVIEW",
            Self::Cancelled => "CANCELLED",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        Some(match value {
            "REQUESTED" => Self::Requested,
            "SEARCHING" => Self::Searching,
            "EVALUATING" => Self::Evaluating,
            "QUEUED" => Self::Queued,
            "DOWNLOADING" => Self::Downloading,
            "DOWNLOADED" => Self::Downloaded,
            "INSPECTING" => Self::Inspecting,
            "IDENTIFIED" => Self::Identified,
            "IMPORTING" => Self::Importing,
            "READY" => Self::Ready,
            "NO_RELEASE_FOUND" => Self::NoReleaseFound,
            "NEEDS_SELECTION" => Self::NeedsSelection,
            "DOWNLOAD_FAILED" => Self::DownloadFailed,
            "IMPORT_FAILED" => Self::ImportFailed,
            "NEEDS_REVIEW" => Self::NeedsReview,
            "CANCELLED" => Self::Cancelled,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Ready
                | Self::NoReleaseFound
                | Self::DownloadFailed
                | Self::ImportFailed
                | Self::Cancelled
        )
    }

    pub fn is_active(self) -> bool {
        !self.is_terminal()
    }

    /// In-flight states that block a second acquisition for the same book and
    /// language variant. READY is excluded: a finished acquisition in one
    /// language must not satisfy a request in another, and an acceptable
    /// owned file is settled before any acquisition is created.
    pub const DUPLICATE_PROTECTED_STATES: [AcquisitionStatus; 11] = [
        AcquisitionStatus::Requested,
        AcquisitionStatus::Searching,
        AcquisitionStatus::Evaluating,
        AcquisitionStatus::Queued,
        AcquisitionStatus::Downloading,
        AcquisitionStatus::Downloaded,
        AcquisitionStatus::Inspecting,
        AcquisitionStatus::Identified,
        AcquisitionStatus::Importing,
        AcquisitionStatus::NeedsSelection,
        AcquisitionStatus::NeedsReview,
    ];

    /// SQL literal list of the statuses that block a second acquisition for a
    /// book variant; shared by duplicate checks, the partial unique index and
    /// Discover.
    pub fn protected_states_sql() -> String {
        Self::DUPLICATE_PROTECTED_STATES
            .iter()
            .map(|status| format!("'{}'", status.as_str()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use AcquisitionStatus::*;
        match self {
            Requested => matches!(next, Searching | DownloadFailed | Cancelled),
            Searching => matches!(
                next,
                Evaluating | NoReleaseFound | DownloadFailed | Cancelled
            ),
            Evaluating => matches!(
                next,
                Queued | NeedsSelection | NoReleaseFound | DownloadFailed | Cancelled
            ),
            Queued => matches!(next, Downloading | DownloadFailed | Cancelled),
            Downloading => matches!(next, Downloaded | DownloadFailed | Cancelled),
            Downloaded => matches!(next, Inspecting | ImportFailed | Cancelled),
            Inspecting => matches!(next, Identified | NeedsReview | ImportFailed | Cancelled),
            Identified => matches!(next, Importing | NeedsReview | ImportFailed | Cancelled),
            Importing => matches!(next, Ready | NeedsReview | ImportFailed | Cancelled),
            NeedsReview => matches!(next, Inspecting | Importing | Cancelled),
            NeedsSelection => matches!(next, Queued | NoReleaseFound | Cancelled),
            // Failed requests can be tried again; the pipeline re-searches and
            // the release blocklist keeps it off the fingerprint that failed.
            NoReleaseFound | DownloadFailed => matches!(next, Requested),
            // A failed import can also be inspected again so a person can
            // pick a specific file out of the download.
            ImportFailed => matches!(next, Requested | Downloaded),
            Ready | Cancelled => false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("cannot transition acquisition from {from:?} to {to:?}")]
pub struct TransitionError {
    pub from: AcquisitionStatus,
    pub to: AcquisitionStatus,
}

pub fn validate_transition(
    from: AcquisitionStatus,
    to: AcquisitionStatus,
) -> Result<(), TransitionError> {
    if from.can_transition_to(to) {
        Ok(())
    } else {
        Err(TransitionError { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_db_strings() {
        let all = [
            AcquisitionStatus::Requested,
            AcquisitionStatus::Searching,
            AcquisitionStatus::Evaluating,
            AcquisitionStatus::Queued,
            AcquisitionStatus::Downloading,
            AcquisitionStatus::Downloaded,
            AcquisitionStatus::Inspecting,
            AcquisitionStatus::Identified,
            AcquisitionStatus::Importing,
            AcquisitionStatus::Ready,
            AcquisitionStatus::NoReleaseFound,
            AcquisitionStatus::NeedsSelection,
            AcquisitionStatus::DownloadFailed,
            AcquisitionStatus::ImportFailed,
            AcquisitionStatus::NeedsReview,
            AcquisitionStatus::Cancelled,
        ];

        for status in all {
            assert_eq!(AcquisitionStatus::from_db(status.as_str()), Some(status));
        }
    }

    #[test]
    fn transition_table_is_explicit() {
        use AcquisitionStatus::*;

        let expected: &[(AcquisitionStatus, &[AcquisitionStatus])] = &[
            (Requested, &[Searching, DownloadFailed, Cancelled]),
            (
                Searching,
                &[Evaluating, NoReleaseFound, DownloadFailed, Cancelled],
            ),
            (
                Evaluating,
                &[
                    Queued,
                    NeedsSelection,
                    NoReleaseFound,
                    DownloadFailed,
                    Cancelled,
                ],
            ),
            (Queued, &[Downloading, DownloadFailed, Cancelled]),
            (Downloading, &[Downloaded, DownloadFailed, Cancelled]),
            (Downloaded, &[Inspecting, ImportFailed, Cancelled]),
            (
                Inspecting,
                &[Identified, NeedsReview, ImportFailed, Cancelled],
            ),
            (
                Identified,
                &[Importing, NeedsReview, ImportFailed, Cancelled],
            ),
            (Importing, &[Ready, NeedsReview, ImportFailed, Cancelled]),
            (Ready, &[]),
            (NeedsReview, &[Inspecting, Importing, Cancelled]),
            (NeedsSelection, &[Queued, NoReleaseFound, Cancelled]),
            (NoReleaseFound, &[Requested]),
            (DownloadFailed, &[Requested]),
            (ImportFailed, &[Requested, Downloaded]),
            (Cancelled, &[]),
        ];

        let all = [
            Requested,
            Searching,
            Evaluating,
            Queued,
            Downloading,
            Downloaded,
            Inspecting,
            Identified,
            Importing,
            Ready,
            NoReleaseFound,
            NeedsSelection,
            DownloadFailed,
            ImportFailed,
            NeedsReview,
            Cancelled,
        ];

        for (from, allowed) in expected {
            for to in all {
                let should_allow = allowed.contains(&to);
                assert_eq!(
                    from.can_transition_to(to),
                    should_allow,
                    "{from:?} -> {to:?} should be {should_allow}"
                );
                assert_eq!(validate_transition(*from, to).is_ok(), should_allow);
            }
        }
    }

    #[test]
    fn failure_states_are_terminal_and_counts_match() {
        let all = [
            AcquisitionStatus::Requested,
            AcquisitionStatus::Searching,
            AcquisitionStatus::Evaluating,
            AcquisitionStatus::Queued,
            AcquisitionStatus::Downloading,
            AcquisitionStatus::Downloaded,
            AcquisitionStatus::Inspecting,
            AcquisitionStatus::Identified,
            AcquisitionStatus::Importing,
            AcquisitionStatus::Ready,
            AcquisitionStatus::NoReleaseFound,
            AcquisitionStatus::NeedsSelection,
            AcquisitionStatus::DownloadFailed,
            AcquisitionStatus::ImportFailed,
            AcquisitionStatus::NeedsReview,
            AcquisitionStatus::Cancelled,
        ];

        for status in all {
            if status.is_terminal() {
                assert!(!status.is_active());
            }
        }

        let active = all.iter().filter(|status| status.is_active()).count();
        assert_eq!(active, 11);
        assert!(AcquisitionStatus::ImportFailed.is_terminal());
        assert!(AcquisitionStatus::Ready.is_terminal());
        assert!(AcquisitionStatus::NeedsReview.is_active());
        assert!(!AcquisitionStatus::Downloaded.is_terminal());
        assert_eq!(AcquisitionStatus::DUPLICATE_PROTECTED_STATES.len(), 11);
    }
}

#[cfg(test)]
mod retry_tests {
    use super::AcquisitionStatus::*;

    #[test]
    fn failed_requests_can_be_reopened_but_finished_ones_cannot() {
        for status in [NoReleaseFound, DownloadFailed, ImportFailed] {
            assert!(status.can_transition_to(Requested));
        }
        assert!(ImportFailed.can_transition_to(Downloaded));
        assert!(!DownloadFailed.can_transition_to(Downloaded));
        assert!(!Ready.can_transition_to(Requested));
        assert!(!Cancelled.can_transition_to(Requested));
        assert!(!Importing.can_transition_to(Requested));
    }
}
