; A multifield variable captures all fields following a fixed prefix.
;; Level: boundary
;; Covers: patterns, multifield-suffix-capture
; Protocol: load, reset, run to quiescence.
(deffacts input (row head a b c))
(defrule observe
  (row head $?values)
  => (printout t (length$ ?values) " " (nth$ 1 ?values) " " (nth$ 3 ?values) crlf))
