; A multifield variable can capture zero fields.
;; Level: boundary
;; Covers: patterns, multifield-empty-capture
; Protocol: load, reset, run to quiescence.
(deffacts input (row))
(defrule observe (row $?values) => (printout t (length$ ?values) crlf))
