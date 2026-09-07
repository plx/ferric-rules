; An anonymous single-field wildcard consumes exactly one field.
;; Level: boundary
;; Covers: patterns, anonymous-single-field
; Protocol: load, reset, run to quiescence.
(deffacts input (row) (row a) (row a b))
(defrule observe (row ?) => (printout t "one field" crlf))
