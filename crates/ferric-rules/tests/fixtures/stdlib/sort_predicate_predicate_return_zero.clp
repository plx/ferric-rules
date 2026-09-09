;; #343 pinned sort behavior: predicate-return-zero
(deffunction exchange (?a ?b) 0)
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort exchange (create$ 3 1 2)) crlf)
)
