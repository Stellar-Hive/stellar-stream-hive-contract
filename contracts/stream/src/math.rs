//! The core streaming math. Kept as free functions, independent of storage
//! and `Env`, so it can be unit tested exhaustively without spinning up a
//! contract environment.
use crate::error::StreamError;
use crate::types::{Stream, StreamStatus};

/// Computes the total value accrued by `stream` as of `now` (a ledger
/// sequence number), ignoring withdrawals and the cliff.
///
/// Formula (exactly as specified, multiply-before-divide, fully checked):
/// ```text
/// elapsed        = min(now, end_ledger) - start_ledger        (clamped to >= 0)
/// total_duration = end_ledger - start_ledger
/// streamed       = total_amount * elapsed / total_duration
/// ```
///
/// If the stream has been cancelled, `now` is ignored in favor of the
/// ledger at which it was cancelled (`stream.cancelled_at_ledger`), so a
/// cancelled stream's streamed amount is frozen forever at the value it
/// held the instant it was cancelled.
pub fn streamed_amount(stream: &Stream, now: u32) -> Result<i128, StreamError> {
    let effective_now = if stream.status == StreamStatus::Cancelled {
        stream.cancelled_at_ledger
    } else {
        now
    };

    if effective_now <= stream.start_ledger {
        return Ok(0);
    }

    let clamped_now = core::cmp::min(effective_now, stream.end_ledger);
    let elapsed = clamped_now.saturating_sub(stream.start_ledger) as i128;
    let total_duration = stream
        .end_ledger
        .checked_sub(stream.start_ledger)
        .ok_or(StreamError::InvalidTimeRange)? as i128;

    if total_duration == 0 {
        return Err(StreamError::InvalidTimeRange);
    }

    // Multiply before divide, using checked i128 arithmetic throughout so
    // there is never silent overflow or precision loss.
    let numerator = stream
        .total_amount
        .checked_mul(elapsed)
        .ok_or(StreamError::MathOverflow)?;
    let streamed = numerator
        .checked_div(total_duration)
        .ok_or(StreamError::MathOverflow)?;

    // Defensive clamp: streamed value can never exceed total_amount
    // (guards against any future edge case in the formula above).
    Ok(core::cmp::min(streamed, stream.total_amount))
}

/// Computes how much the recipient can withdraw right now: the streamed
/// amount minus what has already been withdrawn, or zero if `now` is
/// before the cliff.
pub fn withdrawable_amount(stream: &Stream, now: u32) -> Result<i128, StreamError> {
    if now < stream.cliff_ledger && stream.status != StreamStatus::Cancelled {
        return Ok(0);
    }
    // Even a cancelled stream that never reached its cliff should not be
    // withdrawable, since the recipient never earned anything before the
    // cliff — evaluate the cliff against the ledger the stream stopped at.
    if stream.status == StreamStatus::Cancelled && stream.cancelled_at_ledger < stream.cliff_ledger {
        return Ok(0);
    }

    let streamed = streamed_amount(stream, now)?;
    let withdrawable = streamed
        .checked_sub(stream.withdrawn)
        .ok_or(StreamError::MathOverflow)?;
    Ok(core::cmp::max(withdrawable, 0))
}

/// Derives the live status of a stream at ledger `now`. A stored
/// `Cancelled` status always wins (it's terminal and time-independent);
/// otherwise the status is purely a function of `now` versus the stream's
/// start/end ledgers.
pub fn compute_status(stream: &Stream, now: u32) -> StreamStatus {
    if stream.status == StreamStatus::Cancelled {
        return StreamStatus::Cancelled;
    }
    if now < stream.start_ledger {
        StreamStatus::Pending
    } else if now >= stream.end_ledger {
        StreamStatus::Completed
    } else {
        StreamStatus::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Address, Env};

    fn make_stream(env: &Env, start: u32, end: u32, cliff: u32, total: i128) -> Stream {
        use soroban_sdk::testutils::Address as _;
        Stream {
            id: 1,
            sender: Address::generate(env),
            recipient: Address::generate(env),
            token: Address::generate(env),
            total_amount: total,
            withdrawn: 0,
            start_ledger: start,
            end_ledger: end,
            cliff_ledger: cliff,
            cancellable: true,
            status: StreamStatus::Active,
            created_at: start,
            cancelled_at_ledger: 0,
        }
    }

    #[test]
    fn streamed_is_zero_before_start() {
        let env = Env::default();
        let s = make_stream(&env, 100, 200, 100, 1000);
        assert_eq!(streamed_amount(&s, 50).unwrap(), 0);
        assert_eq!(streamed_amount(&s, 100).unwrap(), 0);
    }

    #[test]
    fn streamed_is_total_at_and_after_end() {
        let env = Env::default();
        let s = make_stream(&env, 100, 200, 100, 1000);
        assert_eq!(streamed_amount(&s, 200).unwrap(), 1000);
        assert_eq!(streamed_amount(&s, 500).unwrap(), 1000);
    }

    #[test]
    fn streamed_is_exact_half_at_midpoint() {
        let env = Env::default();
        let s = make_stream(&env, 0, 1000, 0, 1_000_000);
        assert_eq!(streamed_amount(&s, 500).unwrap(), 500_000);
    }

    #[test]
    fn streamed_handles_non_divisible_amounts_without_overshoot() {
        let env = Env::default();
        // 1000 total over 3 ledgers: at elapsed=1 -> 333, elapsed=2 -> 666.
        let s = make_stream(&env, 0, 3, 0, 1000);
        assert_eq!(streamed_amount(&s, 1).unwrap(), 333);
        assert_eq!(streamed_amount(&s, 2).unwrap(), 666);
        assert_eq!(streamed_amount(&s, 3).unwrap(), 1000);
    }

    #[test]
    fn withdrawable_is_zero_before_cliff_even_if_streaming() {
        let env = Env::default();
        let s = make_stream(&env, 0, 1000, 500, 1_000_000);
        // At ledger 400 the stream is well underway (40%) but cliff is 500.
        assert_eq!(withdrawable_amount(&s, 400).unwrap(), 0);
        assert_eq!(withdrawable_amount(&s, 500).unwrap(), 500_000);
    }

    #[test]
    fn withdrawable_subtracts_already_withdrawn() {
        let env = Env::default();
        let mut s = make_stream(&env, 0, 1000, 0, 1_000_000);
        s.withdrawn = 200_000;
        assert_eq!(withdrawable_amount(&s, 500).unwrap(), 300_000);
    }

    #[test]
    fn cancelled_stream_freezes_streamed_amount() {
        let env = Env::default();
        let mut s = make_stream(&env, 0, 1000, 0, 1_000_000);
        s.status = StreamStatus::Cancelled;
        s.cancelled_at_ledger = 400;
        // Regardless of how far `now` has advanced past cancellation, the
        // streamed amount must stay pinned at the 40% mark.
        assert_eq!(streamed_amount(&s, 400).unwrap(), 400_000);
        assert_eq!(streamed_amount(&s, 900).unwrap(), 400_000);
        assert_eq!(streamed_amount(&s, 10_000).unwrap(), 400_000);
    }

    #[test]
    fn compute_status_transitions() {
        let env = Env::default();
        let s = make_stream(&env, 100, 200, 100, 1000);
        assert_eq!(compute_status(&s, 50), StreamStatus::Pending);
        assert_eq!(compute_status(&s, 150), StreamStatus::Active);
        assert_eq!(compute_status(&s, 200), StreamStatus::Completed);
        assert_eq!(compute_status(&s, 999), StreamStatus::Completed);
    }

    #[test]
    fn streamed_never_exceeds_total_amount() {
        let env = Env::default();
        let s = make_stream(&env, 0, 7, 0, i128::MAX / 2);
        for now in 0..=20u32 {
            let amt = streamed_amount(&s, now).unwrap();
            assert!(amt <= s.total_amount);
        }
    }
}
