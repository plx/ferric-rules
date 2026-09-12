;; #343 pinned sort behavior: predicate-return-empty-multifield
(deffunction exchange (?a ?b) (create$))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort exchange (create$ 3 1 2)) crlf)
)
