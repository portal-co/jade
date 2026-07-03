export const pascal = (s: string) =>
  s.split("_").map((p) => p[0] + p.slice(1).toLowerCase()).join("");

// args types handled at the VM loop level (not dispatched through exec_op): the
// return/suspend ops ("src"/"src_dest") and the jump-family control-transfer ops
// ("jmp"/"condjmp"/"switch_jump") — none of these produce a value via a normal
// op_*()+set() pair, they tell the driver where to continue (or to return).
export const LOOP_LEVEL = new Set(["src", "src_dest", "jmp", "condjmp", "switch_jump"]);
