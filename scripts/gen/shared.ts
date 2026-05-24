export const pascal = (s: string) =>
  s.split("_").map((p) => p[0] + p.slice(1).toLowerCase()).join("");

// args types handled at the VM loop level (not dispatched through exec_op)
export const LOOP_LEVEL = new Set(["src", "src_dest"]);

// args types that embed raw bytecode blocks (handled by exec_block_op, not exec_op)
export const BLOCK_ARGS = new Set(["fixpoint_block", "if_block", "switch_block"]);
