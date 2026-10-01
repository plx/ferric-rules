;; max rejects a multifield operand after evaluating the ones before it.
;; Level: boundary
;; Covers: max
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (max (mark 1 4) (mark 2 8) (mark 3 (create$ 1 2))))
  (printout t "not reached " ?result crlf))
