;; #343 pinned sort behavior: predicate-return-true
(deffunction exchange (?a ?b) TRUE)
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort exchange (create$ 3 1 2)) crlf)
)
