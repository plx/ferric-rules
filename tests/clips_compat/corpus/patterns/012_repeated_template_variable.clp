; A repeated variable correlates two slots of the same template fact.
;; Level: boundary
;; Covers: patterns, repeated-template-variable
; Protocol: load, reset, run to quiescence.
(deftemplate pair (slot left) (slot right))
(deffacts input (pair (left a) (right a)) (pair (left a) (right b)))
(defrule observe
  (pair (right ?value) (left ?value))
  => (printout t ?value crlf))
