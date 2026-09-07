; A multifield variable can capture fields preceding a fixed suffix.
;; Level: boundary
;; Covers: patterns, multifield-prefix-capture
; Protocol: load, reset, run to quiescence.
(deffacts input (row a b tail))
(defrule observe
  (row $?values tail)
  => (printout t (length$ ?values) " " (nth$ 1 ?values) " " (nth$ 2 ?values) crlf))
