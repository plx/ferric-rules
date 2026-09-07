; A multifield variable may occur between fixed prefix and suffix fields.
;; Level: boundary
;; Covers: patterns, multifield-middle-capture
; Protocol: load, reset, run to quiescence.
(deffacts input (row head a b tail))
(defrule observe
  (row head $?values tail)
  => (printout t (length$ ?values) " " (nth$ 1 ?values) " " (nth$ 2 ?values) crlf))
