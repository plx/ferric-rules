;; An error while evaluating the implode$ operand stops the rule.
;; Level: boundary
;; Covers: implode$
(deffunction fail (?digit) (printout t "fail " ?digit crlf) (/ 1 0))
(defrule probe =>
  (bind ?result (implode$ (fail 1)))
  (printout t "not reached " ?result crlf))
