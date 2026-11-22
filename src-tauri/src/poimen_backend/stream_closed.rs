//! The `Closed` records the backend makes itself (a stream that ended without an event).

use poimen_client::ClientError;

use crate::backend::{CloseCode, StreamRecord};

/// The connection of an attached stream ended without a record.
pub(crate) fn lost(err: &ClientError) -> StreamRecord {
    StreamRecord::Closed {
        reason: format!("the connection to poimen-server ended: {err}"),
        exit_code: None,
        code: Some(CloseCode::Lost),
    }
}

/// `detached`: the view closes without a banner.
pub(crate) fn closed_silently() -> StreamRecord {
    StreamRecord::Closed {
        reason: "detached".into(),
        exit_code: None,
        code: None,
    }
}

pub(crate) fn controller_held() -> StreamRecord {
    StreamRecord::Closed {
        reason: "another client controls this terminal".into(),
        exit_code: None,
        code: Some(CloseCode::ControllerHeld),
    }
}

/// The exit record of a read-only attachment says the terminal is its history (#1195), not
/// "killed": its process is gone and no exit code is known.
pub(crate) fn history_ended(record: StreamRecord, read_only: bool) -> StreamRecord {
    match record {
        StreamRecord::Closed {
            code: Some(CloseCode::Exited),
            exit_code,
            ..
        } if read_only => StreamRecord::Closed {
            reason: "the terminal ended; this is its history, read-only".into(),
            exit_code,
            code: Some(CloseCode::Exited),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exited() -> StreamRecord {
        StreamRecord::Closed {
            reason: "the process was killed".into(),
            exit_code: None,
            code: Some(CloseCode::Exited),
        }
    }

    #[test]
    fn a_read_only_exit_says_it_is_the_history() {
        let StreamRecord::Closed { reason, code, .. } = history_ended(exited(), true) else {
            panic!("closed")
        };
        assert!(reason.contains("history"), "{reason}");
        assert!(matches!(code, Some(CloseCode::Exited)));
    }

    #[test]
    fn a_normal_exit_and_other_records_stay() {
        let StreamRecord::Closed { reason, .. } = history_ended(exited(), false) else {
            panic!("closed")
        };
        assert_eq!(reason, "the process was killed");
        assert!(matches!(
            history_ended(StreamRecord::Output { bytes: "x".into() }, true),
            StreamRecord::Output { .. }
        ));
    }
}
