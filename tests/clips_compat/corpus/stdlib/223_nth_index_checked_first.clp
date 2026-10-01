;; nth$ checks its index before it evaluates the multifield.
;; Level: boundary
;; Covers: nth$
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (nth$ (mark 1 red) (mark 2 (create$ a b))))
  (printout t "not reached " ?result crlf))
