; A predicate constraint can filter the field it binds.
;; Level: boundary
;; Covers: patterns, predicate-local
; Protocol: load, reset, run to quiescence.
(deffacts input (value 0) (value 1))
(defrule observe (value ?value&:(> ?value 0)) => (printout t ?value crlf))
