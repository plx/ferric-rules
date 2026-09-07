; An empty ordered pattern excludes facts containing fields.
;; Level: boundary
;; Covers: facts, ordered-empty-pattern-arity
; Protocol: load, reset, run to quiescence.
(deffacts input (row) (row a))
(defrule observe (row) => (printout t "empty" crlf))
