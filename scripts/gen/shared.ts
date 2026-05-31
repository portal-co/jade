export const pascal = (s: string) =>
  s.split("_").map((p) => p[0] + p.slice(1).toLowerCase()).join("");

// args types handled at the VM loop level (not dispatched through exec_op)
export const LOOP_LEVEL = new Set(["src", "src_dest"]);

// args types that embed raw bytecode blocks (control-flow ops dispatched via the
// Ops control-flow methods rather than a direct handler)
export const BLOCK_ARGS = new Set(["while_block", "if_block", "switch_block"]);
