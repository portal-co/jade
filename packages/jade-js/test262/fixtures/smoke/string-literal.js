/*---
description: Smoke — string literal via STR opcode and string identity
info: |
    Synthetic smoke fixture for the Jade test262 runner. Exercises the STR opcode
    (fromCodePoint construction) and === on strings across every environment.
flags: [raw]
---*/

var s = "jade";
if (s === "jade") { return "ok"; }
return "bad";
