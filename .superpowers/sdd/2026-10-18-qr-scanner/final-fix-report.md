# Consolidated final review fixes

Base: ad463c2. Scope: both Important and both requested Minor findings from final-review.md, against the approved design. No broad camera layout/window-count refactor, new benchmark, or physical probe was needed. The original running user-visible debug process was not signalled, stopped, or relaunched.

## Changes

- Successful CaptureFinished retains its Prepared screenshot and batches window restoration with the existing full-image scan task. The task prepares grayscale off the UI thread, emits DecodeRequested, and uses the existing bounded decoder. Optional crop/full rescans remain available.
- Source invalidation is now separate from starting a new scan session. Stop cancels decoder work, signals camera stop and retains the retiring worker for release-before-reopen ordering, clears preview/source state, invalidates late messages, and clears busy without discarding decoded results or scanned status. A subsequent scan or ordinary close still clears results.
- Cancelled capture records distinct non-error feedback, displayed as Capture cancelled.; begin_session clears that feedback for the next operation.
- Runtime measurement emits its JSON diagnostics and then raises CalledProcessError on nonzero primary exit, causing automation to fail. Historical successful measurements are unaffected.
- iced_runtime is a dev-only dependency already present transitively, allowing the capture regression to execute the actual returned Iced task stream without a GUI runtime.

## RED/GREEN evidence

The two Important tests were added before production fixes. Capture failed because no DecodeRequested output appeared; Stop failed with zero retained results instead of one. The cancellation regression was run before adding display/clear behavior and failed with Ready instead of Capture cancelled. The probe regression executes the script with controlled process/procfs/time boundaries and failed because CalledProcessError was not raised for exit 7; it now also verifies emitted diagnostics contain exit 7.

The capture test executes the handler's returned task, verifies full image dimensions and retained screenshot, sends its emitted request through the actual handler/decoder, and observes completed scan state. The Stop test starts with a decoded payload, sends Stop, then delivers an old camera frame (preview plus decode), DecodeRequested and DecodeFinished. Results remain exactly the original payload, preview stays released and busy stays false; the next scan clears the retained list. Existing camera worker stop/drop tests continue covering native-source release mechanics. No physical camera release acceptance is inferred from these unit checks.

## Important RED: cargo test final_review_tests -- --nocapture

```text
   Compiling qr-scanner v0.1.0 (/home/filipw/Personal/qr-scanner-linux/.worktrees/iced-app)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.16s
     Running unittests src/lib.rs (target/debug/deps/qr_scanner-909aa1cb996fc937)

running 2 tests

thread 'app::final_review_tests::stop_retains_results_and_rejects_stale_camera_and_decode_work' (3129241) panicked at src/app/mod.rs:836:9:
assertion `left == right` failed: Stop must retain scanned results
  left: 0
 right: 1
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'app::final_review_tests::captured_image_automatically_schedules_full_decode' (3129240) panicked at src/app/mod.rs:820:9:
capture must schedule decoding without another click
test app::final_review_tests::stop_retains_results_and_rejects_stale_camera_and_decode_work ... FAILED
test app::final_review_tests::captured_image_automatically_schedules_full_decode ... FAILED

failures:

failures:
    app::final_review_tests::captured_image_automatically_schedules_full_decode
    app::final_review_tests::stop_retains_results_and_rejects_stale_camera_and_decode_work

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 37 filtered out; finished in 0.00s

error: test failed, to rerun pass `--lib`
```

## Cancellation RED: cargo test cancellation_feedback_tests

```text
   Compiling qr-scanner v0.1.0 (/home/filipw/Personal/qr-scanner-linux/.worktrees/iced-app)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.91s
     Running unittests src/lib.rs (target/debug/deps/qr_scanner-909aa1cb996fc937)

running 1 test
test app::cancellation_feedback_tests::cancelled_capture_has_distinct_status_cleared_by_next_operation ... FAILED

failures:

---- app::cancellation_feedback_tests::cancelled_capture_has_distinct_status_cleared_by_next_operation stdout ----

thread 'app::cancellation_feedback_tests::cancelled_capture_has_distinct_status_cleared_by_next_operation' (3131078) panicked at src/app/mod.rs:885:9:
assertion `left == right` failed
  left: "Ready"
 right: "Capture cancelled."
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    app::cancellation_feedback_tests::cancelled_capture_has_distinct_status_cleared_by_next_operation

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 39 filtered out; finished in 0.00s

error: test failed, to rerun pass `--lib`
```

## Probe RED: python3 -m unittest discover -s tools -p 'test_measure_runtime.py'

```text
F
======================================================================
FAIL: test_failed_primary_exit_reports_diagnostics_and_fails (test_measure_runtime.ProbeExitTests.test_failed_primary_exit_reports_diagnostics_and_fails)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/home/filipw/Personal/qr-scanner-linux/.worktrees/iced-app/tools/test_measure_runtime.py", line 39, in test_failed_primary_exit_reports_diagnostics_and_fails
    with self.assertRaises(subprocess.CalledProcessError) as error:
AssertionError: CalledProcessError not raised

----------------------------------------------------------------------
Ran 1 test in 0.006s

FAILED (failures=1)
```

## Important GREEN: cargo test final_review_tests

```text
   Compiling qr-scanner v0.1.0 (/home/filipw/Personal/qr-scanner-linux/.worktrees/iced-app)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.12s
     Running unittests src/lib.rs (target/debug/deps/qr_scanner-909aa1cb996fc937)

running 2 tests
test app::final_review_tests::stop_retains_results_and_rejects_stale_camera_and_decode_work ... ok
test app::final_review_tests::captured_image_automatically_schedules_full_decode ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 38 filtered out; finished in 0.00s

     Running unittests src/main.rs (target/debug/deps/qr_scanner-a80c0745ed33fe36)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/decoding.rs (target/debug/deps/decoding-f3aad4d11eef4d2b)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.00s

```

## Cancellation GREEN: cargo test cancellation_feedback_tests

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.17s
     Running unittests src/lib.rs (target/debug/deps/qr_scanner-909aa1cb996fc937)

running 1 test
test app::cancellation_feedback_tests::cancelled_capture_has_distinct_status_cleared_by_next_operation ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 39 filtered out; finished in 0.00s

     Running unittests src/main.rs (target/debug/deps/qr_scanner-a80c0745ed33fe36)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/decoding.rs (target/debug/deps/decoding-f3aad4d11eef4d2b)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.00s

```

## Probe GREEN: python3 -m unittest discover -s tools -p 'test_measure_runtime.py'

```text
.
----------------------------------------------------------------------
Ran 1 test in 0.004s

OK
```

## Final: cargo fmt --check

```text
(no output; successful)
```

## Final: cargo test

```text
   Compiling qr-scanner v0.1.0 (/home/filipw/Personal/qr-scanner-linux/.worktrees/iced-app)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.03s
     Running unittests src/lib.rs (target/debug/deps/qr_scanner-909aa1cb996fc937)

running 40 tests
test app::capture_tests::reset_releases_capture_and_rejects_late_response ... ok
test app::capture_tests::close_releases_capture ... ok
test app::state::decode_tests::ignores_completed_old_session ... ok
test app::state::tests::background_without_host_opens_window ... ok
test app::cancellation_feedback_tests::cancelled_capture_has_distinct_status_cleared_by_next_operation ... ok
test app::final_review_tests::stop_retains_results_and_rejects_stale_camera_and_decode_work ... ok
test app::state::tests::last_window_without_host_exits ... ok
test app::state::tests::losing_host_while_hidden_opens_window ... ok
test app::state::decode_tests::empty_scan_and_failure_are_distinct ... ok
test app::capture_tests::camera_failure_cannot_be_overwritten_by_a_queued_decode ... ok
test app::state::tests::last_window_with_host_stays_alive ... ok
test camera::tests::controllable_clock_limits_preview_and_decode ... ok
test camera::tests::padded_native_rows_are_normalized_and_short_rows_rejected ... ok
test app::capture_tests::camera_frames_after_close_or_stop_cannot_restore_preview ... ok
test app::temporary_capture_tests::source_switch_during_temporary_close_ignores_old_completion ... ok
test app::state::decode_tests::session_results_are_unique_complete_and_cleared ... ok
test camera::tests::malformed_frames_are_rejected ... ok
test capture::crop::tests::scaled_letterboxed_reverse_and_clamped ... ok
test app::temporary_capture_tests::waits_for_every_close_and_restores_after_failure ... ok
test capture::crop::tests::rejects_empty_and_invalid ... ok
test instance::tests::live_listener_without_lock_is_never_unlinked ... ok
test instance::tests::refuses_non_socket_control_path ... ok
test capture::preview::tests::escape_then_reselect_cannot_finish_previous_drag ... ok
test app::temporary_capture_tests::intentional_last_window_close_preserves_capture_session ... ok
test app::final_review_tests::captured_image_automatically_schedules_full_decode ... ok
test capture::tests::adapter_results ... ok
test capture::tests::missing_and_corrupt ... ok
test instance::tests::stale_socket_is_recovered ... ok
test instance::tests::refuses_world_accessible_runtime_directory ... ok
test instance::tests::second_invocation_forwards_without_unlinking_primary ... ok
test camera::tests::stop_and_reopen_release_device_and_clear_preview ... ok
test camera::tests::unplug_releases_source_and_reports_error ... ok
test jobs::tests::cancel_discards_pending_and_active_outcome ... ok
test jobs::tests::serialized_execution_and_bounded_replacement ... ok
test camera::tests::dropping_session_signals_release_without_blocking ... ok
test camera::tests::bounded_latest_frame_replaces_obsolete_preview ... ok
test instance::tests::concurrent_invocations_elect_one_primary ... ok
test camera::tests::worker_schedules_bounded_preview_and_decode_with_fake_clock ... ok
test jobs::tests::stalled_consumer_preserves_each_completed_payload ... ok
test jobs::tests::cancellation_and_shutdown_wake_output_backpressure ... ok

test result: ok. 40 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s

     Running unittests src/main.rs (target/debug/deps/qr_scanner-a80c0745ed33fe36)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/decoding.rs (target/debug/deps/decoding-f3aad4d11eef4d2b)

running 8 tests
test invalid_image ... ok
test safe_urls_only ... ok
test blank_image ... ok
test preserves_non_utf8_bytes ... ok
test single_qr ... ok
test rotated_qr ... ok
test duplicate_codes_are_one_result ... ok
test multiple_qrs ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s

   Doc-tests qr_scanner

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

```

## Final: cargo clippy --all-targets -- -D warnings

```text
    Checking qr-scanner v0.1.0 (/home/filipw/Personal/qr-scanner-linux/.worktrees/iced-app)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.84s
```

## Final: cargo build --release

```text
   Compiling qr-scanner v0.1.0 (/home/filipw/Personal/qr-scanner-linux/.worktrees/iced-app)
    Finished `release` profile [optimized] target(s) in 8.94s
```

## Final assessment and remaining limits

Final verification passed: 40 Rust unit tests, 8 integration tests, 1 Python regression; fmt, clippy and default release build succeeded. git diff --check also succeeded. docs/verification.md reflects the new counts and distinguishes earlier optional GPU checks from this final default-only verification.

The recorded short runtime/decode measurements remain historical observations. Hardware QR/LED/unplug, physical KDE portal/crop/clipboard/Open acceptance and other desktop sessions remain unverified as previously documented. Active decoder computation still cannot be interrupted; worst-case photographic-image shutdown latency remains a follow-up. Deferred duplicate window counts, formatting and replacement-order regression were intentionally not broadened into this fix wave.
