export const opcodes: { [Op in Opcode]: OpcodeInfo } = {
  RET: { id: 0 },
  AWAIT: { id: 1 },
  YIELD: { id: 2 },
  YIELDSTAR: { id: 3 },
  GLOBAL: { id: 4 },
  FN: { id: 5 },
  LIT32: { id: 6 },
  ARR: { id: 7 },
  STR: { id: 8 },
  LITOBJ: { id: 9 },
  NEW_TARGET: { id: 10 },
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
};
