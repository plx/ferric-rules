import type { Fact } from "./ferric";

/**
 * Program that halts on exactly `boundary`, leaving one lower-salience
 * activation and its supporting position fact intact.
 */
export function haltAtActivationSource(boundary: number): string {
  const target = boundary - 1;
  return `
(deffacts start (position 0))
(defrule halt-at-boundary
  (declare (salience 100))
  (position ${target})
  =>
  (halt))
(defrule advance
  ?current <- (position ?n&:(< ?n ${target}))
  =>
  (retract ?current)
  (assert (position (+ ?n 1))))
(defrule after-halt
  (declare (salience -100))
  ?current <- (position ${target})
  =>
  (retract ?current)
  (assert (past-boundary)))
`;
}

/**
 * Emits a non-fatal match diagnostic in the first native chunk, then needs a
 * second 100-activation chunk to drain the agenda.
 */
export const EARLY_DIAGNOSTIC_SOURCE = `
(defrule seed
  (initial-fact)
  =>
  (assert (candidate 1))
  (assert (position 0)))
(defrule bad-match
  (candidate ?value&:(/ 1 0))
  =>
  (assert (must-not-fire)))
(defrule advance
  ?current <- (position ?n&:(< ?n 100))
  =>
  (retract ?current)
  (assert (position (+ ?n 1))))
`;

/**
 * Leaves both a pending halt and an action diagnostic behind. A subsequent
 * zero-limit fresh run must clear both without consuming the agenda.
 */
export const HALT_WITH_DIAGNOSTIC_SOURCE = `
(defrule halt-with-diagnostic
  (declare (salience 100))
  (initial-fact)
  =>
  (assert (candidate 1))
  (halt))
(defrule bad-match
  (candidate ?value&:(/ 1 0))
  =>
  (assert (must-not-fire)))
(defrule after-halt
  (declare (salience -100))
  (initial-fact)
  =>
  (assert (done)))
`;

/** Only the facts that distinguish a preserved halt boundary from overfire. */
export function logicalRunStateFacts(
  facts: readonly Fact[],
): Array<{ relation: string; fields: readonly unknown[] }> {
  return facts
    .filter(
      (fact) => fact.relation === "position" || fact.relation === "past-boundary",
    )
    .map((fact) => ({
      relation: fact.relation!,
      fields: fact.fields,
    }));
}
