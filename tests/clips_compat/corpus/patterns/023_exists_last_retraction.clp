; Retracting the last exists witness cancels its pending activation.
;; Level: interaction
;; Covers: patterns, exists-last-retraction
; Protocol: load, reset, run to quiescence.
(deffacts input (witness) (ready))
(defrule remove-witness
  (declare (salience 10)) ?f <- (witness) => (retract ?f))
(defrule stale (exists (witness)) => (printout t "stale" crlf))
(defrule observe
  (declare (salience -10)) (ready) => (printout t "removed" crlf))
