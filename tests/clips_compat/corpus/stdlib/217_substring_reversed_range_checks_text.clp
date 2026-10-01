;; With a positive end, sub-string evaluates and checks the text even for a reversed range.
;; Level: boundary
;; Covers: sub-string
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (sub-string (mark 1 3) (mark 2 2) (mark 3 42)))
  (printout t "not reached " ?result crlf))
