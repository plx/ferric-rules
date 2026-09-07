; A multifield variable between fixed fields can capture zero fields.
;; Level: boundary
;; Covers: patterns, multifield-middle-empty
; Protocol: load, reset, run to quiescence.
(deffacts input (row head tail))
(defrule observe
  (row head $?values tail)
  => (printout t (length$ ?values) crlf))
