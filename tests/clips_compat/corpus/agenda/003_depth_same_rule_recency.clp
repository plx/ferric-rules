; Default depth strategy prefers the newest match of a single rule.
;; Level: boundary
;; Covers: agenda, depth-same-rule-recency
; Protocol: load, reset, run to quiescence.
(deffacts input (value first) (value second) (value third))
(defrule observe (value ?value) => (printout t ?value crlf))
