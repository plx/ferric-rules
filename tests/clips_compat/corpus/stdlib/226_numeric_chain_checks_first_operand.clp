;; A numeric comparison checks each operand as it evaluates it.
;; Level: boundary
;; Covers: <
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (< (mark 1 abc) (mark 2 1)))
  (printout t "not reached " ?result crlf))
