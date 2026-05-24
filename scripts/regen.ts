import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { writeFileSync } from "node:fs";

import { opcodes, handlers } from "../packages/jade-data/dist/index.js";
import { genVmTs }       from "./gen/vm-ts.ts";
import { genDataRs }     from "./gen/data-rs.ts";
import { genDispatchRs } from "./gen/dispatch-rs.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));

writeFileSync(`${__dirname}/../packages/jade-js/vm.ts`,      genVmTs(opcodes, handlers));
writeFileSync(`${__dirname}/../crates/jade-vm/src/data.rs`,  genDataRs(opcodes));
writeFileSync(`${__dirname}/../crates/jade-vm-core/src/dispatch.rs`, genDispatchRs(opcodes));
