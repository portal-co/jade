/*---
description: try/finally is rejected until jsaw-core's finally lowering covers exceptional paths
info: |
    jsaw-core's Stmt::Try lowering places the finalizer only on the fall-through
    join block, so an uncaught throw (or an early return/break/continue) would
    silently skip it. The frontend rejects try/finally with Unsupported rather
    than miscompile (docs/exceptions-plan.md); this fixture pins the rejection —
    it becomes a real execution fixture when phase 5 lands upstream.
flags: [raw]
---*/

var ran = false;
try {
  ran = true;
} finally {
}
return ran;
