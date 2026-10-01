;; A numeric comparison that has not failed yet reaches, and rejects, a later SYMBOL.
;; Level: boundary
;; Covers: <
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (< (mark 1 1) (mark 2 2) (mark 3 abc) (mark 4 5)))
  (printout t "not reached " ?result crlf))
