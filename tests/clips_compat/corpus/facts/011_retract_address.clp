; Retracting a bound fact address removes its content from working memory.
;; Level: interaction
;; Covers: facts, retract-address
; Protocol: load, reset, run to quiescence.
(deffacts input (ready) (value remove))
(defrule remove-value
  (declare (salience 10)) ?f <- (value remove)
  => (retract ?f))
(defrule observe
  (ready) (not (value remove)) => (printout t "absent" crlf))
