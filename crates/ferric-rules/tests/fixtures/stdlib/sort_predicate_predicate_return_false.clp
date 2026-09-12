;; #343 pinned sort behavior: predicate-return-false
(deffunction exchange (?a ?b) FALSE)
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort exchange (create$ 3 1 2)) crlf)
)
