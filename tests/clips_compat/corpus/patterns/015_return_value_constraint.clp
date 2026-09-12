; A return-value constraint computes the value a later field must match.
;; Level: boundary
;; Covers: patterns, return-value-constraint
; Protocol: load, reset, run to quiescence.
(deffacts input (pair 3 4) (pair 3 5))
(defrule observe (pair ?value =(+ ?value 1)) => (printout t ?value crlf))
