;; An absent nth$ position still requires a multifield second argument.
;; Level: boundary
;; Covers: nth$
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (nth$ (mark 1 3) (mark 2 42)))
  (printout t "not reached " ?result crlf))
