/*---
description: Smoke — comparison operators and if/else control flow
info: |
    Synthetic smoke fixture for the Jade test262 runner. Exercises EQ/LT/LE and
    COND_JMP lowering across the interpreter and all JIT tiers.
flags: [raw]
---*/

var x = 42;
var y = 7;
if (x === y) { return 0; }
if (x < y) { return 1; }
if (x <= y) { return 2; }
return 3;
