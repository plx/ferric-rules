;; Input operands finish before the first predicate invocation.
(defglobal ?*trace* = 0)
(deffunction mark (?digit ?value) (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
(deffunction exchange (?a ?b) (bind ?*trace* (+ (* ?*trace* 10) 4)) (> ?a ?b))
(deffacts startup (go))
(defrule exercise (go) =>
 (printout t (sort (mark 1 exchange) (mark 2 (create$ 3 1)) (mark 3 2)) ":" ?*trace* crlf))
