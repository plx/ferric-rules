; A variable repeated within an ordered fact imposes equality.
;; Level: basic
;; Covers: patterns, repeated-field-variable
; Protocol: load, reset, run to quiescence.
(deffacts input (pair a a) (pair a b))
(defrule observe (pair ?value ?value) => (printout t ?value crlf))
