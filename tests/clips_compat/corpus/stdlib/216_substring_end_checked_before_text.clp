;; sub-string checks its end position before it evaluates the text.
;; Level: boundary
;; Covers: sub-string
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (sub-string (mark 1 0) (mark 2 "bad") (mark 3 "abc")))
  (printout t "not reached " ?result crlf))
