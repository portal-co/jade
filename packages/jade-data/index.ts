export const opcodes: { [Op in Opcode]: OpcodeInfo } = {
  RET: { id: 0, args: 1 },
  AWAIT: { id: 1, args: 1 },
  YIELD: { id: 2, args: 1 },
  YIELDSTAR: { id: 3, args: 1 },
  GLOBAL: { id: 4, args: 0 },
  FN: { id: 5, args: 4 },
  LIT32: { id: 6, args: 1 },
  ARR: { id: 7, args: "array" },
  STR: { id: 8, args: "array" },
  LITOBJ: { id: 9, args: "object" },
  NEW_TARGET: { id: 10, args: 0 },
};
export type Opcode =
  | "RET"
  | "AWAIT"
  | "YIELD"
  | "YIELDSTAR"
  | "GLOBAL"
  | "FN"
  | "LIT32"
  | "ARR"
  | "STR"
  | "LITOBJ"
  | "NEW_TARGET";
export type OpcodeInfo = {
  id: number;
  args: number | "array" | "object";
};
