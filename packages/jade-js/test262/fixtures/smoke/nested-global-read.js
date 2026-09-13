/*---
description: Smoke — nested function bodies run tenant ops (global member reads) with the parent's tenant
info: |
    Regression: the TS interpreter's FN handler built the child frame's context
    without threading `tenant`, so any tenant op (a GET from a global member read,
    here) inside a nested function crashed with "Cannot read properties of
    undefined (reading 'driveTenant')".
flags: [raw]
---*/

function g() {
  // A free global read is a GET on the realm global — a tenant op.
  print;
  return 7;
}
if (g() !== 7) return 1;
return 0;
