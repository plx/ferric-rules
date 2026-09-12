; Retracting and reasserting equal content gives a rule a fresh activation.
;; Level: interaction
;; Covers: agenda, retract-reassert-refires
; Protocol: load, reset, run to quiescence.
(deffacts input (value a) (cycle))
(defrule observe
  (declare (salience 10)) (value ?value) => (printout t ?value crlf))
(defrule cycle-value
  ?control <- (cycle) ?f <- (value a)
  => (retract ?control ?f) (assert (value a)))
