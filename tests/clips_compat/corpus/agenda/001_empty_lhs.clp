; An empty LHS creates an activation after reset and fires once.
;; Level: basic
;; Covers: agenda, empty-lhs
; Protocol: load, reset, run to quiescence.
(defrule observe => (printout t "start" crlf))
