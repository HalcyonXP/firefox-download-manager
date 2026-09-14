# HTTP fixture retirement

`download-manager-test-server` is a loopback-only test dependency, not a production HTTP server or package payload.

## Ownership and observation boundaries

- **Request ledger:** records a parsed request after the observation gate. It does not timestamp client initiation or immediate header arrival.
- **Connection start/join counts:** retained handler threads and completed joins, including failed joins. Counts alone do not assert successful thread exits, request delivery or engine retirement.
- **Fixture retirement:** stops acceptance, releases observation/response barriers, requests shutdown of independently owned socket clones, wakes configured stalls, stops body generation between chunks, and joins every handler and the listener. Successful `retire()` returns the joined handler count. Errors remain errors on repeated calls.

The listener retains a socket clone before creating each handler and retains its `JoinHandle` immediately upon return. Completed handlers are joined and reaped during acceptance instead of accumulating handles for the full fixture lifetime. Partial creation failure and listener unwinding retain cleanup responsibility. A panicked handler does not skip sibling joins or become a successful retirement. Socket shutdown and barrier release are requests to unblock work, not joins.

`TestServer::drop` performs retirement if it has not already been observed explicitly. An unexpected retirement error fails the test unless it is already unwinding. An explicitly observed retirement error is not rethrown on drop. Configured live stalls, request I/O deadlines and intake concurrency are unchanged; stop notification interrupts fixture stalls only during retirement.

Dropping a response-pause guard while the server remains live permits normal response delivery. Dropping the **server** instead interrupts owned I/O and joins handlers; a complete reply is not promised. These are separate test cases.

## Counterexamples and coverage

A controlled test against the previous fixture observed one active response handler after server drop returned. The old handler's fixed stall and activity-guard release were contained before asserting, and the enclosing test process was waited. This is not retrospective per-handler join evidence. With retained retirement the same test observes zero active handlers at return.

A separate real TCP case holds a fully received request before ledger insertion, shuts down and drops its sole client, then retires the server. The ledger grows from zero to one after the client is gone. This rejects the premise that later ledger growth necessarily means new client work. It does **not** establish the cause of a historical CI failure.

Focused cases cover incomplete headers, both barriers, live stalls, a handler tail beyond socket shutdown, stopped generation without socket shutdown, listener unwind, failed-but-joined handlers with a live sibling, repeated retirement and completed-handle reaping. The engine shutdown case now observes local request admission at zero, refuses resumed coordinator admission, checks stable Paused/progress observations across the unchanged 200 ms interval, and independently reopens the state store. It does not use remote ledger timing as local worker-lifetime authority or assert arbitrary kernel/network quiescence.

At `ca33e5c`, CI34713355127 failed the former shutdown ledger assertion (6→8), while package/dependency jobs passed. At `9376696`, CI34718828251 passed all four jobs with the former fixture/assertion. Neither observation diagnoses the earlier failure. The retention correction is test infrastructure and evidence-boundary work, not a production engine fix, installed-browser acceptance or transfer of qualification to new artifacts.

Local workspace formatting/Clippy/tests/build/dependency checks, npm and all269 Python cases pass. The reviewed LLVM/UCRT target passes all9 ownership cases,9 adversarial cases,4 unchanged native-fixture cases and34 engine lifecycle cases. Five mutations reject omitted joins, forgotten join failures, omitted unwind cleanup, omitted reaping and omitted body-stop checks; sources were restored before final checks. This evidence remains component-only.
