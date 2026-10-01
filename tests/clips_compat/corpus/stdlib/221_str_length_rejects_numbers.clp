;; str-length rejects a number after evaluating it once.
;; Level: boundary
;; Covers: str-length
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (str-length (mark 1 42)))
  (printout t "not reached " ?result crlf))
