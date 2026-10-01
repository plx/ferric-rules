;; sub-string checks each position as it evaluates it: a bad start stops the later arguments.
;; Level: boundary
;; Covers: sub-string
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (sub-string (mark 1 "bad") (mark 2 2) (mark 3 "abc")))
  (printout t "not reached " ?result crlf))
