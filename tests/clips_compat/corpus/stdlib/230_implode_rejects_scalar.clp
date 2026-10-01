;; implode$ rejects a scalar after evaluating it once.
;; Level: boundary
;; Covers: implode$
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (implode$ (mark 1 abc)))
  (printout t "not reached " ?result crlf))
