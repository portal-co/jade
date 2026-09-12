/*---
description: Smoke — numeric literal round-trips through the pipeline
info: |
    Synthetic smoke fixture for the Jade test262 runner. Not a real test262 test;
    exists so Phase 0 can prove the full pipeline (frontmatter parse, frontend compile,
    interpret, JIT tiers, differential compare) without needing the harness primordials.
flags: [raw]
---*/

var x = 42;
return x;
