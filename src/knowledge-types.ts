export type EvaluationKind = "target" | "paraphrase" | "indirect" | "locality";
export type EvaluationCase = {
  kind: EvaluationKind;
  prompt: string;
  expected: string;
  forbidden: string;
  matchMode: "exact" | "contains";
  aliases: string[];
  language: "und" | "en" | "ja" | "zh";
};
