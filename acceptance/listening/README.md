# Native audio listening review

This is a protocol, not a completed receipt. Run it only once the candidate has
an audible native output path. The current software-device model has no
CoreAudio sink, so no listening pass is claimed.

The reviewer must be a person. Record their identity, UTC date, clean candidate
commit, SHA-256 of the actual running binary, macOS version and physical output
device. Record the original reference used (the reviewer's build-606 session or
an explicitly described expectation) and any platform-profile assumptions.

Use the P01–P30 rows in `acceptance/matrix.json` as the complete feature list.
For each row, record the scenario and whether it has audible behavior. Any
not-applicable decision needs a specific reason and the reviewer's agreement;
it does not excuse a missing audio implementation. Applicable rows must include
menu/UI feedback, sound options and persistence, engine/world sound, music,
race starts, collisions, horn and nitro where the feature supplies them.

For every applicable item:

1. Start the named scene with the retained deterministic input/session profile.
2. Verify the expected sound source is audible on the physical device. Include
   playback start/stop, volume/pan/spatial placement, looping and transitions
   relevant to that feature. Check a quiet interval for stuck voices.
3. Record pass, fail or not-heard and concrete notes. Not-heard fails.
4. Retain the corresponding original and port DirectSound stream comparison and
   its first divergence, if any. A listening result cannot excuse a stream
   mismatch; stream equality cannot substitute for hearing the native output.

Every required applicable row must pass on the same candidate binary and source
revision. Rebuilds or source changes invalidate the receipt. A machine or agent
must never fill the reviewer's identity, observations or pass results. Keep
receipts local until an executable receipt validator and the independent final
review are implemented; this protocol does not promote S6, G7 or P28.
