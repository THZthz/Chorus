// golden-dump artifact — the fuzz action selector, frozen verbatim from
// tools/golden-dump.ts (review-1 appendix 4(a): the selector source ships with
// the goldens so the Rust mirror can be diffed against it letter by letter).
// Ported 1:1 to rust/tests/common/reach_fuzz_lib.rs as choose_step; the LCG is
// js::Lcg / tests/common/reach_fuzz_lib.rs (same discipline as
// reach-fuzz-lib.ts rng — XOR 0x9e3779b9 seeding, u32 wrapping multiply-add).
// Caller discipline (frozen): one selector instance per seed seeded rng(seed);
// per attempt draw once over the full candidate list; a rejected settleStep
// consumes the draw and retries; K=25 applied steps or the 400-attempt cap.
export function chooseStep<T>(rand: () => number, steps: T[]): T {
  return steps[Math.floor(rand() * steps.length)]!
}
