;; An empty member$ needle still requires a multifield second argument.
;; Level: boundary
;; Covers: member$
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule probe =>
  (bind ?result (member$ (mark 1 (create$)) (mark 2 42)))
  (printout t "not reached " ?result crlf))
