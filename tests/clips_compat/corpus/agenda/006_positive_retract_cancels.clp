; Removing a positive supporting fact cancels its pending activation.
;; Level: interaction
;; Covers: agenda, positive-retract-cancels
; Protocol: load, reset, run to quiescence.
(deffacts input (ready) (value a))
(defrule remove-value
  (declare (salience 10)) ?f <- (value a) => (retract ?f))
(defrule stale (value a) => (printout t "stale" crlf))
(defrule observe
  (declare (salience -10)) (ready) => (printout t "removed" crlf))
