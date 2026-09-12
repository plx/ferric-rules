; Asserting a blocker cancels an already-created negative activation.
;; Level: interaction
;; Covers: patterns, not-assert-cancels
; Protocol: load, reset, run to quiescence.
(deffacts input (ready))
(defrule block
  (declare (salience 10)) (ready) => (assert (blocked)))
(defrule stale (ready) (not (blocked)) => (printout t "stale" crlf))
(defrule observe
  (declare (salience -10)) (blocked) => (printout t "blocked" crlf))
