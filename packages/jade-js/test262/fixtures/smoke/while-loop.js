/*---
description: Smoke — while loop through the jump-based block structure
info: |
    Synthetic smoke fixture for the Jade test262 runner. Exercises loop lowering
    (COND_JMP back-edge) — the construct where Tier 0's dispatch loop, Tier 1's reloop
    restructuring, and Tier 2's CFG reconstruction most plausibly disagree.
flags: [raw]
---*/

var x = true;
while (x) {
  x = false;
}
return x;
