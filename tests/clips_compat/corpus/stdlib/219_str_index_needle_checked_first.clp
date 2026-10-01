;; str-index checks its needle before it evaluates the haystack.
;; Level: boundary
;; Covers: str-index
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (str-index (mark 1 42) (mark 2 "abc")))
  (printout t "not reached " ?result crlf))
