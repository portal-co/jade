export const pascal = (s: string) =>
  s.split("_").map((p) => p[0] + p.slice(1).toLowerCase()).join("");

// args types handled at the VM loop level (not dispatched through exec_op): the
// return/suspend ops ("src"/"src_dest"), the jump-family control-transfer ops
// ("jmp"/"condjmp"/"switch_jump"), and the exception ops ("throw_src"/
// "trypush"/"none") — none of these produce a value via a normal op_*()+set()
// pair, they tell the driver where to continue (or raise/dispatch exceptions).
export const LOOP_LEVEL = new Set(["src", "src_dest", "jmp", "condjmp", "switch_jump", "throw_src", "trypush", "none"]);
