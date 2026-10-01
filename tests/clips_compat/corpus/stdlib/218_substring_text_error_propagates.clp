;; An error while evaluating the sub-string text stops the rule.
;; Level: boundary
;; Covers: sub-string
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(deffunction fail (?digit) (printout t "fail " ?digit crlf) (/ 1 0))
(defrule probe =>
  (bind ?result (sub-string (mark 1 0) (mark 2 2) (fail 3)))
  (printout t "not reached " ?result crlf))
