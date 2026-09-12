; Removing the final negative blocker enables a rule.
;; Level: interaction
;; Covers: patterns, not-retract-unblocks
; Protocol: load, reset, run to quiescence.
(deffacts input (ready) (blocked))
(defrule unblock
  (declare (salience 10)) ?blocker <- (blocked) => (retract ?blocker))
(defrule observe (ready) (not (blocked)) => (printout t "unblocked" crlf))
