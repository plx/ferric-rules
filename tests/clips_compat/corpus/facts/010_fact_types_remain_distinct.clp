; Integer and float facts with equal numeric values are distinct facts.
;; Level: boundary
;; Covers: facts, fact-types-remain-distinct
; Protocol: load, reset, run to quiescence.
(deffacts input (value 7) (value 7.0))
(defrule integer-value
  (declare (salience 10)) (value 7) => (printout t "integer" crlf))
(defrule float-value (value 7.0) => (printout t "float" crlf))
