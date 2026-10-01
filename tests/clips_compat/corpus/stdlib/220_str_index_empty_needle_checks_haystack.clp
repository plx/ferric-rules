;; An empty str-index needle still evaluates and checks the haystack.
;; Level: boundary
;; Covers: str-index
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (str-index (mark 1 "") (mark 2 1.5)))
  (printout t "not reached " ?result crlf))
